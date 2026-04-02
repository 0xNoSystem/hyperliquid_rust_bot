use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};

use arrow::array::{Float64Array, UInt64Array};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::record_batch::RecordBatch;
use log::info;
use parquet::arrow::ArrowWriter;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::basic::{Compression, ZstdLevel};
use parquet::file::properties::WriterProperties;
use tokio::sync::{Mutex as TokioMutex, OwnedMutexGuard, watch};

use crate::Price;

/// Schema for candle parquet files.
fn candle_schema() -> Schema {
    Schema::new(vec![
        Field::new("ts", DataType::UInt64, false),
        Field::new("open", DataType::Float64, false),
        Field::new("high", DataType::Float64, false),
        Field::new("low", DataType::Float64, false),
        Field::new("close", DataType::Float64, false),
        Field::new("volume", DataType::Float64, false),
        Field::new("close_time", DataType::UInt64, false),
    ])
}

/// Convert a slice of `Price` into an Arrow `RecordBatch`.
fn prices_to_batch(prices: &[Price]) -> RecordBatch {
    let schema = Arc::new(candle_schema());
    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(UInt64Array::from_iter_values(
                prices.iter().map(|p| p.open_time),
            )),
            Arc::new(Float64Array::from_iter_values(
                prices.iter().map(|p| p.open),
            )),
            Arc::new(Float64Array::from_iter_values(
                prices.iter().map(|p| p.high),
            )),
            Arc::new(Float64Array::from_iter_values(prices.iter().map(|p| p.low))),
            Arc::new(Float64Array::from_iter_values(
                prices.iter().map(|p| p.close),
            )),
            Arc::new(Float64Array::from_iter_values(prices.iter().map(|p| p.vlm))),
            Arc::new(UInt64Array::from_iter_values(
                prices.iter().map(|p| p.close_time),
            )),
        ],
    )
    .expect("schema matches arrays")
}

/// Read all `Price` rows from a parquet file.
fn read_all_prices(path: &Path) -> Vec<Price> {
    let file = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };

    let builder = match ParquetRecordBatchReaderBuilder::try_new(file) {
        Ok(b) => b,
        Err(_) => return Vec::new(),
    };

    let reader = match builder.build() {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };

    let mut prices = Vec::new();
    for batch in reader.flatten() {
        let ts = batch
            .column(0)
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        let open = batch
            .column(1)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let high = batch
            .column(2)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let low = batch
            .column(3)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let close = batch
            .column(4)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let volume = batch
            .column(5)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let close_time = batch
            .column(6)
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();

        for i in 0..batch.num_rows() {
            prices.push(Price {
                open_time: ts.value(i),
                open: open.value(i),
                high: high.value(i),
                low: low.value(i),
                close: close.value(i),
                vlm: volume.value(i),
                close_time: close_time.value(i),
            });
        }
    }
    prices
}

/// Write `prices` to a parquet file (overwrites if exists).
fn write_prices(path: &Path, prices: &[Price]) -> Result<(), String> {
    if prices.is_empty() {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create candle directory: {e}"))?;
    }

    let batch = prices_to_batch(prices);
    let props = WriterProperties::builder()
        .set_compression(Compression::ZSTD(ZstdLevel::try_new(3).unwrap()))
        .build();

    let file = fs::File::create(path).map_err(|e| format!("failed to create parquet file: {e}"))?;
    let mut writer = ArrowWriter::try_new(file, batch.schema(), Some(props))
        .map_err(|e| format!("failed to create arrow writer: {e}"))?;
    writer
        .write(&batch)
        .map_err(|e| format!("failed to write batch: {e}"))?;
    writer
        .close()
        .map_err(|e| format!("failed to close writer: {e}"))?;

    Ok(())
}

/// Per-key state: serialises writes and broadcasts fetch progress to waiters.
struct KeyState {
    /// Held for the duration of a fetch — only one fetcher per key at a time.
    /// Wrapped in `Arc` so we can obtain `OwnedMutexGuard` (no lifetime issues).
    lock: Arc<TokioMutex<()>>,
    /// (loaded, total) progress. Waiters subscribe via `watch::Receiver`.
    progress: watch::Sender<(u64, u64)>,
}

impl KeyState {
    fn new() -> Self {
        let (tx, _) = watch::channel((0u64, 0u64));
        Self {
            lock: Arc::new(TokioMutex::new(())),
            progress: tx,
        }
    }
}

/// Persistent candle cache backed by Parquet files.
/// One file per (exchange, market, asset, quote, timeframe) combination.
///
/// Thread safety: per-key locking serialises writes. When a second fetcher
/// requests the same key that is already being fetched, it subscribes to
/// progress updates from the first fetcher and reads from cache once done.
pub struct CandleStore {
    base_dir: PathBuf,
    keys: StdMutex<HashMap<String, Arc<KeyState>>>,
}

impl CandleStore {
    /// Open (or create) the candle store directory.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let base_dir = path.as_ref().to_path_buf();
        fs::create_dir_all(&base_dir)
            .map_err(|e| format!("failed to create candle store directory: {e}"))?;

        info!("Candle store opened at {}", base_dir.display());

        Ok(Self {
            base_dir,
            keys: StdMutex::new(HashMap::new()),
        })
    }

    /// Get or create the `KeyState` for a candle key.
    fn key_state(&self, key: &CandleKey) -> Arc<KeyState> {
        let name = self.key_name(key);
        let mut map = self.keys.lock().unwrap();
        map.entry(name)
            .or_insert_with(|| Arc::new(KeyState::new()))
            .clone()
    }

    /// Derive the unique string name for a key (matches the parquet filename stem).
    fn key_name(&self, key: &CandleKey) -> String {
        format!(
            "{}_{}_{}_{}",
            key.exchange, key.market, key.asset_quote, key.tf,
        )
    }

    /// Resolve the parquet file path for a given candle key.
    fn file_path(&self, key: &CandleKey) -> PathBuf {
        self.base_dir
            .join(format!("{}.parquet", self.key_name(key)))
    }

    /// Acquire the per-key lock. Returns `KeyGuard` which provides:
    /// - `is_first()`: true if you acquired immediately (you should fetch)
    /// - `progress_rx()`: a watch receiver to monitor another fetcher's progress
    /// - `send_progress(loaded, total)`: broadcast progress to waiters
    ///
    /// If the lock is already held, this method subscribes to progress updates
    /// and relays them through `on_progress` until the lock is released.
    /// After that it returns with `is_first() == false`.
    pub async fn acquire_key<F>(&self, key: &CandleKey, mut on_progress: F) -> KeyGuard
    where
        F: FnMut(u64, u64),
    {
        let state = self.key_state(key);

        // Try the lock without blocking first.
        if let Ok(guard) = state.lock.clone().try_lock_owned() {
            // Reset progress for new fetch
            let _ = state.progress.send((0, 0));
            return KeyGuard {
                _guard: Some(guard),
                state: state.clone(),
                first: true,
            };
        }

        // Lock is held — someone else is fetching. Subscribe and relay progress.
        let mut rx = state.progress.subscribe();
        info!(
            "candle key {} is being fetched by another task, waiting…",
            self.key_name(key)
        );

        loop {
            // Relay current value
            let (loaded, total) = *rx.borrow_and_update();
            if total > 0 {
                on_progress(loaded, total);
            }

            tokio::select! {
                // Wait for a new progress tick
                changed = rx.changed() => {
                    if changed.is_err() {
                        // Sender dropped — the other fetcher finished or panicked
                        break;
                    }
                }
                // Also try to acquire the lock (it might be released between ticks)
                guard = state.lock.clone().lock_owned() => {
                    // We got the lock — the other fetcher is done.
                    // Relay final progress
                    let (loaded, total) = *rx.borrow();
                    if total > 0 {
                        on_progress(loaded, total);
                    }
                    drop(guard);
                    break;
                }
            }
        }

        KeyGuard {
            _guard: None,
            state: state.clone(),
            first: false,
        }
    }

    /// Insert candles, merging with existing data and deduplicating by timestamp.
    /// Caller should hold the key lock via `acquire_key`.
    pub fn insert_many(&self, key: &CandleKey, new_prices: &[Price]) {
        if new_prices.is_empty() {
            return;
        }

        let path = self.file_path(key);

        // Read existing candles
        let mut all = read_all_prices(&path);

        // Collect existing timestamps for dedup
        let existing_ts: BTreeSet<u64> = all.iter().map(|p| p.open_time).collect();

        // Append only new timestamps
        for p in new_prices {
            if !existing_ts.contains(&p.open_time) {
                all.push(*p);
            }
        }

        // Sort by timestamp
        all.sort_unstable_by_key(|p| p.open_time);

        // Write back
        if let Err(e) = write_prices(&path, &all) {
            log::warn!("candle_store insert_many failed: {e}");
        }
    }

    /// Return all cached candles in `[start, end)` ordered by timestamp.
    pub fn range_to_vec(&self, key: &CandleKey, start: u64, end: u64) -> Vec<Price> {
        let path = self.file_path(key);
        let all = read_all_prices(&path);
        all.into_iter()
            .filter(|p| p.open_time >= start && p.open_time < end)
            .collect()
    }

    /// Count how many candle timestamps we have cached in `[start, end)`.
    pub fn count_range(&self, key: &CandleKey, start: u64, end: u64) -> u64 {
        let path = self.file_path(key);
        let all = read_all_prices(&path);
        all.iter()
            .filter(|p| p.open_time >= start && p.open_time < end)
            .count() as u64
    }

    /// Identify missing segments in `[normalized_start, normalized_end)` by
    /// checking which expected timestamps are absent.
    /// Returns `(missing_segments, cached_count)`.
    pub fn find_missing(
        &self,
        key: &CandleKey,
        normalized_start: u64,
        normalized_end: u64,
        candle_interval_ms: u64,
    ) -> (Vec<MissingSegment>, u64) {
        let path = self.file_path(key);
        let all = read_all_prices(&path);

        // Build set of cached timestamps in range
        let cached_ts: BTreeSet<u64> = all
            .iter()
            .filter(|p| p.open_time >= normalized_start && p.open_time < normalized_end)
            .map(|p| p.open_time)
            .collect();

        let cached_count = cached_ts.len() as u64;

        // Walk expected timestamps, collect gaps
        let mut missing = Vec::new();
        let mut gap_start: Option<u64> = None;
        let mut ts = normalized_start;

        while ts < normalized_end {
            if cached_ts.contains(&ts) {
                if let Some(start) = gap_start.take() {
                    missing.push(MissingSegment { start, end: ts });
                }
            } else if gap_start.is_none() {
                gap_start = Some(ts);
            }
            ts = ts.saturating_add(candle_interval_ms);
        }

        if let Some(start) = gap_start {
            missing.push(MissingSegment {
                start,
                end: normalized_end,
            });
        }

        (missing, cached_count)
    }

    /// Full cache lookup matching the `CandleCache::lookup_range` API.
    /// Returns cached candles if complete, otherwise the missing segments.
    pub fn lookup_range(
        &self,
        key: &CandleKey,
        normalized_start: u64,
        normalized_end: u64,
        candle_interval_ms: u64,
    ) -> CacheLookup {
        let (missing, cached_in_range) =
            self.find_missing(key, normalized_start, normalized_end, candle_interval_ms);

        let cached = if missing.is_empty() && cached_in_range > 0 {
            Some(self.range_to_vec(key, normalized_start, normalized_end))
        } else {
            None
        };

        CacheLookup {
            missing,
            cached,
            cached_in_range,
        }
    }
}

/// RAII guard returned by `acquire_key`.
pub struct KeyGuard {
    _guard: Option<OwnedMutexGuard<()>>,
    state: Arc<KeyState>,
    first: bool,
}

impl KeyGuard {
    /// True if this caller acquired the lock first (should do the fetch).
    /// False if another task already fetched — data should be in cache.
    pub fn is_first(&self) -> bool {
        self.first
    }

    /// Broadcast progress to any waiters. Only meaningful when `is_first()`.
    pub fn send_progress(&self, loaded: u64, total: u64) {
        let _ = self.state.progress.send((loaded, total));
    }
}

/// Composite key identifying a candle series.
#[derive(Debug, Clone)]
pub struct CandleKey {
    pub exchange: String,
    pub market: String,
    pub asset_quote: String,
    pub tf: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MissingSegment {
    pub start: u64,
    pub end: u64,
}

pub struct CacheLookup {
    pub missing: Vec<MissingSegment>,
    pub cached: Option<Vec<Price>>,
    pub cached_in_range: u64,
}
