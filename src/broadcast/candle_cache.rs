use super::PriceData;
use crate::{CandleHistory, Error, HL_MAX_CANDLES, Price, TimeFrame, TimeFrameData, load_candles};
use hyperliquid_rust_sdk::{BaseUrl, InfoClient};
use rustc_hash::FxHasher;
use std::collections::HashMap;
use std::hash::BuildHasherDefault;
use std::sync::Arc;
use tokio::sync::{
    mpsc::{Receiver, Sender},
    oneshot,
};

type FxMap<K, V> = HashMap<K, V, BuildHasherDefault<FxHasher>>;

pub type CandleCount = u32;

pub struct CandleSnapshotRequest {
    pub asset: String,
    pub request: HashMap<TimeFrame, CandleCount>,
    pub reply: oneshot::Sender<Result<TimeFrameData, Error>>,
}

pub enum CacheCmdIn {
    NewFeed {
        asset: String,
        rx: tokio::sync::broadcast::Receiver<PriceData>,
    },
    DropFeed(String),
    Price {
        asset: String,
        data: PriceData,
    },
    Snapshot(CandleSnapshotRequest),
    Backfill {
        asset: String,
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
    candles: FxMap<String, FxMap<TimeFrame, CandleHistory>>,
    builders: FxMap<String, HashMap<TimeFrame, TfBuilder>>,
}

impl CandleCache {
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
            request.request.retain(|tf, count| {
                if let Some(history) = tf_map.get(tf)
                    && !history.is_empty()
                {
                    let take = (*count as usize).min(history.len());
                    let data: Vec<Price> = history.iter().rev().take(take).rev().copied().collect();
                    cached.insert(*tf, data);
                    return false;
                }
                true
            });
        }
        cached
    }

    fn add_feed(&mut self, asset: String, rx: tokio::sync::broadcast::Receiver<PriceData>) {
        let builders: HashMap<TimeFrame, TfBuilder> = TimeFrame::available_tfs()
            .into_iter()
            .map(|tf| (tf, TfBuilder::new(tf)))
            .collect();
        self.builders.insert(asset.clone(), builders);
        self.candles.entry(asset.clone()).or_default();

        let cmd_tx = self.cmd_tx.clone();
        tokio::spawn(async move {
            let mut rx = rx;
            while let Ok(data) = rx.recv().await {
                let _ = cmd_tx.try_send(CacheCmdIn::Price {
                    asset: asset.clone(),
                    data,
                });
            }
        });
    }

    fn drop_feed(&mut self, asset: &String) {
        self.candles.remove(asset);
        self.builders.remove(asset);
    }

    fn digest(&mut self, asset: &String, data: PriceData) {
        match data {
            PriceData::Single(price) => self.digest_single(asset, price),
            PriceData::Bulk(prices) => {
                for price in prices {
                    self.digest_single(asset, price);
                }
            }
        }
    }

    fn digest_single(&mut self, asset: &String, price: Price) {
        if let Some(builders) = self.builders.get_mut(asset) {
            let tf_map = self.candles.entry(asset.clone()).or_default();
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
        let asset = request.asset.clone();

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

            // Backfill BEFORE reply — ensures cache is queued for update
            // before the caller can trigger another Snapshot request
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

    fn handle_backfill(&mut self, asset: String, data: TimeFrameData) {
        let tf_map = self.candles.entry(asset.clone()).or_default();
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
                CacheCmdIn::Price { asset, data } => self.digest(&asset, data),
                CacheCmdIn::Snapshot(request) => self.fetch_candles(request),
                CacheCmdIn::Backfill { asset, data } => self.handle_backfill(asset, data),
            }
        }
    }
}
