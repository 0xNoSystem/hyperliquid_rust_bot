use super::{PriceAsset, PriceData};
use crate::helper::info_client_with_timeout;
use crate::{
    CandleHistory, Error, HL_MAX_CANDLES, Price, TimeFrame, TimeFrameData, load_candles, metrics,
};
use hyperliquid_rust_sdk::{BaseUrl, InfoClient};
use rustc_hash::FxHasher;
use std::collections::{HashMap, HashSet};
use std::hash::BuildHasherDefault;
use std::sync::Arc;
use tokio::sync::{
    mpsc::error::TrySendError,
    mpsc::{Receiver, Sender},
    oneshot,
};
use tokio_util::sync::CancellationToken;

type FxMap<K, V> = HashMap<K, V, BuildHasherDefault<FxHasher>>;

pub type CandleCount = u32;

pub struct CandleSnapshotRequest {
    pub asset: Arc<str>,
    pub request: HashMap<TimeFrame, CandleCount>,
    pub reply: oneshot::Sender<Result<TimeFrameData, Error>>,
}

pub enum CacheCmdIn {
    NewFeed {
        asset: Arc<str>,
        rx: tokio::sync::broadcast::Receiver<PriceData>,
    },
    DropFeed(Arc<str>),
    Price(PriceAsset),
    Snapshot(CandleSnapshotRequest),
    Backfill {
        asset: Arc<str>,
        data: TimeFrameData,
    },
    FetchComplete {
        asset: Arc<str>,
        data: TimeFrameData,
        completed: HashSet<TimeFrame>,
    },
}

const CACHE_CHANNEL_SIZE: usize = 1024;
const CACHE_INTERNAL_SEND_TIMEOUT_SECS: u64 = 5;

struct TfBuilder {
    tf: TimeFrame,
    prev_close: Option<u64>,
    next_close: Option<u64>,
}

impl TfBuilder {
    fn new(tf: TimeFrame) -> Self {
        TfBuilder {
            tf,
            prev_close: None,
            next_close: None,
        }
    }

    #[inline]
    fn digest(&mut self, price: Price) -> bool {
        let ts = price.close_time;
        let tf_ms = self.tf.to_millis();

        let mut next = match self.next_close {
            Some(n) => n,
            None => {
                self.next_close = Some(ts);
                return false;
            }
        };

        if ts > next {
            while ts >= next {
                self.prev_close = Some(next);
                next += tf_ms;
            }
            self.next_close = Some(next);
            true
        } else {
            false
        }
    }

    #[inline]
    fn resync(&mut self, last_candle_time: u64) {
        let tf_ms = self.tf.to_millis();
        let prev_close = (last_candle_time / tf_ms) * tf_ms;
        self.prev_close = Some(prev_close);
        self.next_close = Some(prev_close + tf_ms);
    }
}

pub struct CandleCache {
    info_client: Arc<InfoClient>,
    cmd_tx: Sender<CacheCmdIn>,
    cmd_rx: Receiver<CacheCmdIn>,
    candles: FxMap<Arc<str>, FxMap<TimeFrame, CandleHistory>>,
    fetched: FxMap<Arc<str>, HashSet<TimeFrame>>,
    inflight: FxMap<Arc<str>, HashSet<TimeFrame>>,
    pending_snapshots: Vec<PendingSnapshot>,
    builders: FxMap<Arc<str>, HashMap<TimeFrame, TfBuilder>>,
}

struct PendingSnapshot {
    asset: Arc<str>,
    request: HashMap<TimeFrame, CandleCount>,
    cached: TimeFrameData,
    reply: oneshot::Sender<Result<TimeFrameData, Error>>,
}

impl CandleCache {
    fn cached_slice(history: &CandleHistory, count: CandleCount) -> Option<Vec<Price>> {
        let need = count as usize;
        if history.len() < need {
            return None;
        }

        Some(
            history
                .iter()
                .rev()
                .take(need)
                .rev()
                .copied()
                .collect::<Vec<_>>(),
        )
    }

    fn cached_or_fetched_slice(
        history: Option<&CandleHistory>,
        already_fetched: bool,
        count: CandleCount,
    ) -> Option<Vec<Price>> {
        if let Some(history) = history {
            if let Some(data) = Self::cached_slice(history, count) {
                return Some(data);
            }

            if already_fetched {
                return Some(history.iter().copied().collect());
            }
        } else if already_fetched {
            return Some(Vec::new());
        }

        None
    }

    pub async fn new(url: BaseUrl) -> Result<(Self, Sender<CacheCmdIn>), Error> {
        let info_client = Arc::new(info_client_with_timeout("candle cache", url).await?);
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel(CACHE_CHANNEL_SIZE);
        Ok((
            CandleCache {
                info_client,
                cmd_tx: cmd_tx.clone(),
                cmd_rx,
                candles: HashMap::default(),
                fetched: HashMap::default(),
                inflight: HashMap::default(),
                pending_snapshots: Vec::new(),
                builders: HashMap::default(),
            },
            cmd_tx,
        ))
    }

    fn try_cache(&self, request: &mut CandleSnapshotRequest) -> TimeFrameData {
        let mut cached: TimeFrameData = HashMap::default();
        if let Some(tf_map) = self.candles.get(&request.asset) {
            let asset = Arc::clone(&request.asset);
            request.request.retain(|tf, count| {
                let already_fetched = self
                    .fetched
                    .get(&asset)
                    .is_some_and(|fetched| fetched.contains(tf));

                if let Some(data) =
                    Self::cached_or_fetched_slice(tf_map.get(tf), already_fetched, *count)
                {
                    cached.insert(*tf, data);
                    return false;
                }

                if let Some(history) = tf_map.get(tf)
                    && !history.is_empty()
                {
                    log::info!(
                        "candle cache partial for {} {:?}: have {}, need {}; fetching backfill",
                        asset,
                        tf,
                        history.len(),
                        count
                    );
                }
                true
            });
        }
        cached
    }

    fn add_feed(&mut self, asset: Arc<str>, rx: tokio::sync::broadcast::Receiver<PriceData>) {
        let builders: HashMap<TimeFrame, TfBuilder> = TimeFrame::available_tfs()
            .into_iter()
            .map(|tf| (tf, TfBuilder::new(tf)))
            .collect();
        self.builders.insert(Arc::clone(&asset), builders);
        self.candles.entry(Arc::clone(&asset)).or_default();

        let cmd_tx = self.cmd_tx.clone();
        tokio::spawn(async move {
            let mut rx = rx;
            let mut queue_full = false;
            let mut dropped = 0_u64;
            while let Ok(data) = rx.recv().await {
                match cmd_tx.try_send(CacheCmdIn::Price((Arc::clone(&asset), data))) {
                    Ok(()) => {
                        if queue_full {
                            log::info!(
                                "candle cache queue recovered for {} after dropping {} updates",
                                &asset,
                                dropped
                            );
                            queue_full = false;
                            dropped = 0;
                        }
                    }
                    Err(TrySendError::Full(_)) => {
                        metrics::inc_candle_cache_price_dropped();
                        dropped = dropped.saturating_add(1);
                        if !queue_full {
                            log::warn!(
                                "candle cache queue full for {}; dropping live candle updates until it drains",
                                &asset
                            );
                            queue_full = true;
                        }
                    }
                    Err(TrySendError::Closed(_)) => {
                        log::warn!("candle cache queue closed for {}", &asset);
                        break;
                    }
                }
            }
        });
    }

    fn drop_feed(&mut self, asset: &Arc<str>) {
        self.candles.remove(asset);
        self.fetched.remove(asset);
        self.inflight.remove(asset);
        self.builders.remove(asset);

        let mut pending = std::mem::take(&mut self.pending_snapshots);
        for snapshot in pending.drain(..) {
            if snapshot.asset == *asset {
                let _ = snapshot.reply.send(Err(Error::Custom(format!(
                    "candle feed dropped for {}",
                    asset
                ))));
            } else {
                self.pending_snapshots.push(snapshot);
            }
        }
    }

    fn digest(&mut self, asset: &Arc<str>, data: PriceData) {
        match data {
            PriceData::Single(price) => self.digest_single(asset, price),
            PriceData::Bulk(prices) => {
                for price in prices {
                    self.digest_single(asset, price);
                }
            }
        }
    }

    fn digest_single(&mut self, asset: &Arc<str>, price: Price) {
        if let Some(builders) = self.builders.get_mut(asset) {
            let tf_map = self.candles.entry(Arc::clone(asset)).or_default();
            for (tf, builder) in builders.iter_mut() {
                if builder.digest(price) {
                    tf_map
                        .entry(*tf)
                        .or_insert_with(|| Box::new(arraydeque::ArrayDeque::default()))
                        .push_back(price);
                }
            }
        }
    }

    fn fetch_candles(&mut self, mut request: CandleSnapshotRequest) {
        let cached = self.try_cache(&mut request);

        if request.request.is_empty() {
            let _ = request.reply.send(Ok(cached));
            return;
        }

        let asset = Arc::clone(&request.asset);
        let fetch_request = self.register_snapshot_request(&request);
        self.pending_snapshots.push(PendingSnapshot {
            asset: request.asset,
            request: request.request,
            cached,
            reply: request.reply,
        });

        if fetch_request.is_empty() {
            return;
        }

        self.spawn_fetch(asset, fetch_request);
    }

    fn register_snapshot_request(
        &mut self,
        request: &CandleSnapshotRequest,
    ) -> HashMap<TimeFrame, CandleCount> {
        let inflight = self.inflight.entry(Arc::clone(&request.asset)).or_default();
        register_requested_timeframes(inflight, &request.request)
    }

    fn spawn_fetch(&self, asset: Arc<str>, request: HashMap<TimeFrame, CandleCount>) {
        let client = self.info_client.clone();
        let cmd_tx = self.cmd_tx.clone();

        tokio::spawn(async move {
            let mut backfill: TimeFrameData = HashMap::default();
            let mut completed = HashSet::new();

            for tf in request.keys() {
                log::info!("Fetching {:?} candles for {}", tf, &asset);
                let mut last_err = None;
                for attempt in 0..3 {
                    match load_candles(&client, &asset, *tf, HL_MAX_CANDLES).await {
                        Ok(data) => {
                            if data.is_empty() {
                                log::warn!(
                                    "candle fetch returned no candles for {} {:?}",
                                    &asset,
                                    tf
                                );
                            }
                            backfill.insert(*tf, data);
                            completed.insert(*tf);
                            last_err = None;
                            break;
                        }
                        Err(e) => {
                            log::warn!(
                                "backfill failed for {} {:?} (attempt {}): {:?}",
                                &asset,
                                tf,
                                attempt + 1,
                                e
                            );
                            last_err = Some(format!("{e:?}"));
                            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        }
                    }
                }
                if let Some(err) = last_err {
                    log::error!(
                        "candle fetch exhausted retries for {} {:?}: {}",
                        &asset,
                        tf,
                        err
                    );
                    completed.insert(*tf);
                }
            }

            let complete = CacheCmdIn::FetchComplete {
                asset: Arc::clone(&asset),
                data: backfill,
                completed,
            };
            match tokio::time::timeout(
                std::time::Duration::from_secs(CACHE_INTERNAL_SEND_TIMEOUT_SECS),
                cmd_tx.send(complete),
            )
            .await
            {
                Ok(Ok(())) => {}
                Ok(Err(_)) => {
                    log::warn!("candle cache channel closed before completing fetch for {asset}");
                }
                Err(_) => {
                    log::warn!("timed out queuing candle fetch completion for {asset}");
                }
            }
        });
    }
}

fn register_requested_timeframes(
    inflight: &mut HashSet<TimeFrame>,
    request: &HashMap<TimeFrame, CandleCount>,
) -> HashMap<TimeFrame, CandleCount> {
    let mut fetch_request = HashMap::new();

    for (&tf, &count) in request {
        if inflight.insert(tf) {
            fetch_request.insert(tf, count);
        }
    }

    fetch_request
}

impl CandleCache {
    fn handle_backfill(&mut self, asset: Arc<str>, data: TimeFrameData) {
        let tf_map = self.candles.entry(Arc::clone(&asset)).or_default();
        let fetched = self.fetched.entry(Arc::clone(&asset)).or_default();
        let mut builders = self.builders.get_mut(&asset);

        for (tf, candles) in data {
            fetched.insert(tf);

            if let Some(last) = candles.last()
                && let Some(builder) = builders.as_mut().and_then(|b| b.get_mut(&tf))
            {
                builder.resync(last.close_time);
            }

            let history: CandleHistory = Box::new(candles.into_iter().collect());
            tf_map.insert(tf, history);
        }
    }

    fn handle_fetch_complete(
        &mut self,
        asset: Arc<str>,
        data: TimeFrameData,
        completed: HashSet<TimeFrame>,
    ) {
        if !self.builders.contains_key(&asset)
            && !self
                .pending_snapshots
                .iter()
                .any(|snapshot| snapshot.asset == asset)
        {
            self.inflight.remove(&asset);
            return;
        }

        if !data.is_empty() {
            self.handle_backfill(Arc::clone(&asset), data);
        }

        if let Some(inflight) = self.inflight.get_mut(&asset) {
            for tf in &completed {
                inflight.remove(tf);
            }
            if inflight.is_empty() {
                self.inflight.remove(&asset);
            }
        }

        self.resolve_pending_snapshots(&asset, &completed);
    }

    fn resolve_pending_snapshots(&mut self, asset: &Arc<str>, completed: &HashSet<TimeFrame>) {
        let mut pending = std::mem::take(&mut self.pending_snapshots);

        for mut snapshot in pending.drain(..) {
            if snapshot.asset == *asset {
                self.fill_cached_snapshot(&mut snapshot);
                for tf in completed {
                    snapshot.request.remove(tf);
                }
            }

            if snapshot.request.is_empty() {
                let _ = snapshot.reply.send(Ok(snapshot.cached));
            } else {
                self.pending_snapshots.push(snapshot);
            }
        }
    }

    fn fill_cached_snapshot(&self, snapshot: &mut PendingSnapshot) {
        let Some(tf_map) = self.candles.get(&snapshot.asset) else {
            return;
        };
        let fetched = self.fetched.get(&snapshot.asset);

        snapshot.request.retain(|tf, count| {
            let already_fetched = fetched.is_some_and(|fetched| fetched.contains(tf));
            if let Some(data) =
                Self::cached_or_fetched_slice(tf_map.get(tf), already_fetched, *count)
            {
                snapshot.cached.insert(*tf, data);
                false
            } else {
                true
            }
        });
    }
}

impl CandleCache {
    pub async fn start(&mut self, shutdown: CancellationToken) {
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => {
                    break;
                }
                maybe_cmd = self.cmd_rx.recv() => {
                    let Some(cmd) = maybe_cmd else {
                        break;
                    };
                    match cmd {
                        CacheCmdIn::NewFeed { asset, rx } => self.add_feed(asset, rx),
                        CacheCmdIn::DropFeed(asset) => self.drop_feed(&asset),
                        CacheCmdIn::Price((asset, data)) => self.digest(&asset, data),
                        CacheCmdIn::Snapshot(request) => self.fetch_candles(request),
                        CacheCmdIn::Backfill { asset, data } => self.handle_backfill(asset, data),
                        CacheCmdIn::FetchComplete {
                            asset,
                            data,
                            completed,
                        } => self.handle_fetch_complete(asset, data, completed),
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CandleCache, register_requested_timeframes};
    use crate::{CandleHistory, Price, TimeFrame};
    use std::collections::{HashMap, HashSet};

    fn price(close_time: u64) -> Price {
        Price {
            open: close_time as f64,
            high: close_time as f64,
            low: close_time as f64,
            close: close_time as f64,
            open_time: close_time.saturating_sub(60_000),
            close_time,
            vlm: 1.0,
        }
    }

    #[test]
    fn cached_slice_requires_full_requested_count() {
        let history: CandleHistory = Box::new([price(1_000), price(2_000)].into_iter().collect());

        assert!(CandleCache::cached_slice(&history, 3).is_none());
    }

    #[test]
    fn cached_slice_returns_latest_requested_candles() {
        let history: CandleHistory = Box::new(
            [price(1_000), price(2_000), price(3_000), price(4_000)]
                .into_iter()
                .collect(),
        );

        let cached = CandleCache::cached_slice(&history, 2).expect("cache hit");

        assert_eq!(cached.len(), 2);
        assert_eq!(cached[0].close_time, 3_000);
        assert_eq!(cached[1].close_time, 4_000);
    }

    #[test]
    fn cached_or_fetched_slice_returns_short_authoritative_history() {
        let history: CandleHistory = Box::new([price(1_000), price(2_000)].into_iter().collect());

        let cached = CandleCache::cached_or_fetched_slice(Some(&history), true, 5)
            .expect("fetched cache hit");

        assert_eq!(cached.len(), 2);
        assert_eq!(cached[0].close_time, 1_000);
        assert_eq!(cached[1].close_time, 2_000);
    }

    #[test]
    fn cached_or_fetched_slice_misses_short_unfetched_history() {
        let history: CandleHistory = Box::new([price(1_000), price(2_000)].into_iter().collect());

        assert!(CandleCache::cached_or_fetched_slice(Some(&history), false, 5).is_none());
    }

    #[test]
    fn register_requested_timeframes_only_fetches_not_inflight() {
        let mut inflight = HashSet::from([TimeFrame::Min1]);
        let request = HashMap::from([(TimeFrame::Min1, 5000), (TimeFrame::Hour1, 5000)]);

        let fetch = register_requested_timeframes(&mut inflight, &request);

        assert_eq!(fetch.len(), 1);
        assert_eq!(fetch.get(&TimeFrame::Hour1), Some(&5000));
        assert!(inflight.contains(&TimeFrame::Min1));
        assert!(inflight.contains(&TimeFrame::Hour1));
    }
}
