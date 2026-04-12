use super::{PriceAsset, PriceData};
use crate::{CandleHistory, Error, HL_MAX_CANDLES, Price, TimeFrame, TimeFrameData, load_candles};
use hyperliquid_rust_sdk::{BaseUrl, InfoClient};
use rustc_hash::FxHasher;
use std::collections::HashMap;
use std::hash::BuildHasherDefault;
use std::sync::Arc;
use tokio::sync::{
    mpsc::error::TrySendError,
    mpsc::{Receiver, Sender},
    oneshot,
};

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
}

const CACHE_CHANNEL_SIZE: usize = 1024;

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
        self.prev_close = Some((last_candle_time / tf_ms) * tf_ms);
        self.next_close = Some(self.prev_close.unwrap() + tf_ms);
    }
}

pub struct CandleCache {
    info_client: Arc<InfoClient>,
    cmd_tx: Sender<CacheCmdIn>,
    cmd_rx: Receiver<CacheCmdIn>,
    candles: FxMap<Arc<str>, FxMap<TimeFrame, CandleHistory>>,
    builders: FxMap<Arc<str>, HashMap<TimeFrame, TfBuilder>>,
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

    pub async fn new(url: BaseUrl) -> Result<(Self, Sender<CacheCmdIn>), Error> {
        let info_client = Arc::new(InfoClient::new(None, Some(url)).await?);
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel(CACHE_CHANNEL_SIZE);
        Ok((
            CandleCache {
                info_client,
                cmd_tx: cmd_tx.clone(),
                cmd_rx,
                candles: HashMap::default(),
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
                if let Some(history) = tf_map.get(tf) {
                    if let Some(data) = Self::cached_slice(history, *count) {
                        cached.insert(*tf, data);
                        return false;
                    }

                    if !history.is_empty() {
                        log::info!(
                            "candle cache partial for {} {:?}: have {}, need {}; fetching backfill",
                            asset,
                            tf,
                            history.len(),
                            count
                        );
                    }
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
            while let Ok(data) = rx.recv().await {
                match cmd_tx.try_send(CacheCmdIn::Price((Arc::clone(&asset), data))) {
                    Ok(()) => {
                        if queue_full {
                            log::info!("candle cache queue recovered for {}", &asset);
                            queue_full = false;
                        }
                    }
                    Err(TrySendError::Full(_)) => {
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
        self.builders.remove(asset);
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

    fn fetch_candles(&self, mut request: CandleSnapshotRequest) {
        let cached = self.try_cache(&mut request);

        if request.request.is_empty() {
            let _ = request.reply.send(Ok(cached));
            return;
        }

        let client = self.info_client.clone();
        let cmd_tx = self.cmd_tx.clone();
        let asset = Arc::clone(&request.asset);

        tokio::spawn(async move {
            let mut result: TimeFrameData = cached;
            let mut backfill: TimeFrameData = HashMap::default();

            for (tf, count) in &request.request {
                log::info!("Fetching {:?} candles for {}", tf, &asset);
                let mut last_err = None;
                for attempt in 0..3 {
                    match load_candles(&client, &asset, *tf, HL_MAX_CANDLES).await {
                        Ok(data) if data.is_empty() => {
                            log::warn!(
                                "candle fetch returned empty for {} {:?} (attempt {})",
                                &asset,
                                tf,
                                attempt + 1
                            );
                            last_err = Some("empty response from HL".to_string());
                            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        }
                        Ok(data) => {
                            let reply_data = if data.len() > *count as usize {
                                data[data.len() - *count as usize..].to_vec()
                            } else {
                                data.clone()
                            };
                            result.insert(*tf, reply_data);
                            backfill.insert(*tf, data);
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
                }
            }

            if !backfill.is_empty() {
                let _ = cmd_tx
                    .send(CacheCmdIn::Backfill {
                        asset,
                        data: backfill,
                    })
                    .await;
            }

            let _ = request.reply.send(Ok(result));
        });
    }

    fn handle_backfill(&mut self, asset: Arc<str>, data: TimeFrameData) {
        let tf_map = self.candles.entry(Arc::clone(&asset)).or_default();
        let mut builders = self.builders.get_mut(&asset);

        for (tf, candles) in data {
            if let Some(last) = candles.last()
                && let Some(builder) = builders.as_mut().and_then(|b| b.get_mut(&tf))
            {
                builder.resync(last.close_time);
            }

            let history: CandleHistory = Box::new(candles.into_iter().collect());
            tf_map.insert(tf, history);
        }
    }
}

impl CandleCache {
    pub async fn start(&mut self) {
        while let Some(cmd) = self.cmd_rx.recv().await {
            match cmd {
                CacheCmdIn::NewFeed { asset, rx } => self.add_feed(asset, rx),
                CacheCmdIn::DropFeed(asset) => self.drop_feed(&asset),
                CacheCmdIn::Price((asset, data)) => self.digest(&asset, data),
                CacheCmdIn::Snapshot(request) => self.fetch_candles(request),
                CacheCmdIn::Backfill { asset, data } => self.handle_backfill(asset, data),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CandleCache;
    use crate::{CandleHistory, Price};

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
}
