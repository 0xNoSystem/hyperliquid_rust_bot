use super::CacheCmdIn;
use super::PriceData;
use crate::{
    Error, MAX_DISCONNECTION_WINDOW, TimeFrame, candles_snapshot, get_all_assets, get_time_now,
    parse_candle, subscribe_candles,
};
use hyperliquid_rust_sdk::{AssetMeta, BaseUrl, InfoClient, Message};
use rustc_hash::FxHasher;
use std::collections::{HashMap, HashSet};
use std::hash::BuildHasherDefault;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{
    Mutex, broadcast,
    mpsc::{Sender, UnboundedReceiver, UnboundedSender, unbounded_channel},
    oneshot,
};
use tokio::time::{Duration, interval};

const SUBSCRIPTION_ATTEMPTS_MAX: u32 = 5;
const OI_UNSUB_BELOW: f64 = 100_000.0;
const ASSET_CONTEXT_REFRESH_SECS: u64 = 12 * 60 * 60;

type FxMap<K, V> = HashMap<K, V, BuildHasherDefault<FxHasher>>;
pub type SubReply = Result<SubscriptionReply, Error>;

pub struct SubscriptionReply {
    pub px_receiver: broadcast::Receiver<PriceData>,
    pub meta: AssetMeta,
}

pub struct SubscribePayload {
    pub asset: Arc<str>,
    pub reply: oneshot::Sender<SubReply>,
}

pub enum BroadcastCmd {
    Subscribe(SubscribePayload),
    Unsubscribe(Arc<str>),
    SetSubId { asset: Arc<str>, sub_id: u32 },
    CleanUp(Arc<str>),
}

struct AssetFeed {
    tx: broadcast::Sender<PriceData>,
    sub_id: Option<u32>,
}

pub struct Broadcaster {
    url: BaseUrl,
    info_client: Arc<Mutex<InfoClient>>,
    cmd_tx: UnboundedSender<BroadcastCmd>,
    cmd_rx: UnboundedReceiver<BroadcastCmd>,
    channels: FxMap<Arc<str>, AssetFeed>,
    pending_unsubscribes: HashSet<Arc<str>, BuildHasherDefault<FxHasher>>,
    asset_contexts: FxMap<Arc<str>, f64>,
    cache_tx: Sender<CacheCmdIn>,
    universe: Vec<AssetMeta>,
}

impl Broadcaster {
    pub async fn new(
        url: BaseUrl,
        cache_tx: Sender<CacheCmdIn>,
    ) -> Result<(Self, UnboundedSender<BroadcastCmd>), Error> {
        let info_client = InfoClient::with_reconnect(None, Some(url)).await?;
        let universe = get_all_assets(&info_client).await?;
        let (cmd_tx, cmd_rx) = unbounded_channel::<BroadcastCmd>();
        Ok((
            Broadcaster {
                url,
                info_client: Arc::new(Mutex::new(info_client)),
                cmd_tx: cmd_tx.clone(),
                cmd_rx,
                channels: HashMap::default(),
                pending_unsubscribes: HashSet::default(),
                asset_contexts: HashMap::default(),
                cache_tx,
                universe,
            },
            cmd_tx,
        ))
    }

    async fn add_sub(&mut self, sub_req: SubscribePayload) -> Result<(), Error> {
        let asset = sub_req.asset;

        let meta = match self.universe.iter().find(|a| a.name == *asset) {
            Some(m) => m.clone(),
            None => {
                let _ = sub_req.reply.send(Err(Error::AssetNotFound));
                return Ok(());
            }
        };

        let rx = if let Some(feed) = self.channels.get(&asset) {
            feed.tx.subscribe()
        } else {
            let (tx, rx) = broadcast::channel::<PriceData>(256);
            self.cache_tx
                .send(CacheCmdIn::NewFeed {
                    asset: Arc::clone(&asset),
                    rx: tx.subscribe(),
                })
                .await
                .map_err(|_| Error::Custom("CandleCache channel closed".into()))?;
            self.channels.insert(
                Arc::clone(&asset),
                AssetFeed {
                    tx: tx.clone(),
                    sub_id: None,
                },
            );

            let info_client = self.info_client.clone();
            let cmd_tx = self.cmd_tx.clone();
            let asset = Arc::clone(&asset);
            let url = self.url;

            tokio::spawn(async move {
                if let Err(e) =
                    spawn_hl_feed(info_client, cmd_tx, Arc::clone(&asset), tx, url).await
                {
                    log::error!("HL feed for {} exited with error: {:?}", &asset, e);
                }
            });

            rx
        };

        let _ = sub_req.reply.send(Ok(SubscriptionReply {
            px_receiver: rx,
            meta,
        }));

        Ok(())
    }

    async fn drop_cache_feed(&self, asset: Arc<str>) {
        if self
            .cache_tx
            .send(CacheCmdIn::DropFeed(asset.clone()))
            .await
            .is_err()
        {
            log::warn!("failed to drop candle cache feed for {}", &asset);
        }
    }

    fn spawn_remote_unsubscribe(&self, asset: Arc<str>, sub_id: u32) {
        let client = self.info_client.clone();
        tokio::spawn(async move {
            let mut client = client.lock().await;
            if let Err(e) = client.unsubscribe(sub_id).await {
                log::warn!(
                    "failed to unsubscribe {} (sub_id {}): {:?}",
                    &asset,
                    sub_id,
                    e
                );
            }
        });
    }

    async fn unsubscribe_from_feed(&mut self, asset: Arc<str>) {
        if let Some(feed) = self.channels.remove(&asset) {
            self.drop_cache_feed(Arc::clone(&asset)).await;
            if let Some(sub_id) = feed.sub_id {
                self.spawn_remote_unsubscribe(asset, sub_id);
            } else {
                self.pending_unsubscribes.insert(asset);
            }
        }
    }

    #[inline]
    fn is_feed_idle(&self, asset: &str) -> bool {
        self.channels
            .get(asset)
            .map(|feed| feed.tx.receiver_count() == 0)
            .unwrap_or(true)
    }

    #[inline]
    fn set_sub_id(&mut self, asset: &str, sub_id: u32) {
        if let Some(feed) = self.channels.get_mut(asset) {
            feed.sub_id = Some(sub_id);
        } else {
            let asset = Arc::<str>::from(asset);
            if self.pending_unsubscribes.remove(&asset) {
                self.spawn_remote_unsubscribe(asset, sub_id);
            }
        }
    }

    fn update_asset_contexts_for_meta(
        asset_contexts: &mut FxMap<Arc<str>, f64>,
        meta: hyperliquid_rust_sdk::Meta,
        contexts: Vec<hyperliquid_rust_sdk::AssetContext>,
    ) {
        for (asset, context) in meta.universe.into_iter().zip(contexts) {
            match context.open_interest.parse::<f64>() {
                Ok(open_interest) => {
                    asset_contexts.insert(Arc::from(asset.name), open_interest);
                }
                Err(e) => {
                    log::warn!("failed to parse open interest for {}: {}", asset.name, e);
                }
            }
        }
    }

    async fn refresh_asset_contexts(&mut self) -> Result<(), Error> {
        let info_client = InfoClient::new(None, Some(self.url)).await?;
        let mut asset_contexts = FxMap::default();

        let (meta, contexts) = info_client.meta_and_asset_contexts().await?;
        Self::update_asset_contexts_for_meta(&mut asset_contexts, meta, contexts);

        for dex in info_client.perp_dexs().await?.into_iter().flatten() {
            let (meta, contexts) = info_client
                .meta_and_asset_contexts_for_dex(dex.name.clone())
                .await?;
            Self::update_asset_contexts_for_meta(&mut asset_contexts, meta, contexts);
        }

        self.asset_contexts = asset_contexts;
        Ok(())
    }

    pub async fn start(&mut self) {
        if let Err(e) = self.refresh_asset_contexts().await {
            log::warn!("failed to refresh asset contexts on startup: {:?}", e);
        }

        let mut asset_context_refresh = interval(Duration::from_secs(ASSET_CONTEXT_REFRESH_SECS));
        // Consume the immediate first tick so the interval starts its 12h countdown.
        // Without this, the first select! iteration would double-refresh.
        asset_context_refresh.tick().await;

        loop {
            tokio::select! {
                _ = asset_context_refresh.tick() => {
                    if let Err(e) = self.refresh_asset_contexts().await {
                        log::warn!("failed to refresh asset contexts: {:?}", e);
                    }
                }
                maybe_cmd = self.cmd_rx.recv() => {
                    let Some(cmd) = maybe_cmd else {
                        break;
                    };
                    match cmd {
                        BroadcastCmd::Subscribe(payload) => {
                            if let Err(e) = self.add_sub(payload).await {
                                log::error!("failed to add subscription: {:?}", e);
                            }
                        }
                        BroadcastCmd::Unsubscribe(asset) => {
                            if self.is_feed_idle(&asset)
                                && self
                                    .asset_contexts
                                    .get(&asset)
                                    .is_some_and(|oi| *oi < OI_UNSUB_BELOW)
                            {
                                self.unsubscribe_from_feed(asset).await;
                            }
                        }
                        BroadcastCmd::SetSubId { asset, sub_id } => self.set_sub_id(&asset, sub_id),
                        BroadcastCmd::CleanUp(asset) => {
                            log::warn!("cleaning up dead feed for {}", &asset);
                            self.channels.remove(&asset);
                            self.pending_unsubscribes.remove(&asset);
                            self.drop_cache_feed(asset).await;
                        }
                    }
                }
            }
        }
    }
}

async fn spawn_hl_feed(
    info_client: Arc<Mutex<InfoClient>>,
    cmd_tx: UnboundedSender<BroadcastCmd>,
    asset: Arc<str>,
    tx: broadcast::Sender<PriceData>,
    url: BaseUrl,
) -> Result<(), Error> {
    let mut attempts = 0;
    let (sub_id, mut price_rx) = loop {
        let result = {
            let mut client = info_client.lock().await;
            subscribe_candles(&mut client, &asset).await
        };

        match result {
            Ok(sub) => break sub,
            Err(e) => {
                attempts += 1;
                if attempts >= SUBSCRIPTION_ATTEMPTS_MAX {
                    return Err(e);
                }
                log::warn!(
                    "Failed to subscribe to {} (attempt {}/{}): {:?}",
                    &asset,
                    attempts,
                    SUBSCRIPTION_ATTEMPTS_MAX,
                    e
                );
            }
        }
    };

    let _ = cmd_tx.send(BroadcastCmd::SetSubId {
        asset: Arc::clone(&asset),
        sub_id,
    });

    let mut disconnected = false;
    let mut disconnection_start: Option<Instant> = None;
    let mut last_confirmed_close: Option<u64> = None;

    while let Some(msg) = price_rx.recv().await {
        match msg {
            Message::Candle(candle) => match parse_candle(candle.data) {
                Ok(price) => {
                    if disconnected {
                        disconnected = false;
                        if let Some(timer) = disconnection_start.take()
                            && timer.elapsed().as_millis() > MAX_DISCONNECTION_WINDOW
                        {
                            let disc_start = last_confirmed_close.unwrap_or_else(get_time_now);
                            let end = get_time_now();

                            let fetch_client = InfoClient::new(None, Some(url)).await;

                            if let Ok(client) = fetch_client {
                                match candles_snapshot(
                                    &client,
                                    &asset,
                                    TimeFrame::Min1,
                                    disc_start,
                                    end,
                                )
                                .await
                                {
                                    Ok(missed) if !missed.is_empty() => {
                                        log::info!(
                                            "recovered {} missed 1m candles for {}",
                                            missed.len(),
                                            &asset
                                        );
                                        let _ = tx.send(PriceData::Bulk(missed));
                                    }
                                    Ok(_) => {}
                                    Err(e) => {
                                        log::warn!(
                                            "failed to fetch missed window for {}: {:?}",
                                            &asset,
                                            e
                                        );
                                    }
                                }
                            }
                        }
                    }
                    last_confirmed_close = Some(price.open_time);
                    let _ = tx.send(PriceData::Single(price));
                }
                Err(e) => log::warn!("malformed candle for {}: {:?}", &asset, e),
            },
            Message::NoData if !disconnected => {
                disconnected = true;
                disconnection_start = Some(Instant::now());
                log::info!("{} price stream disconnected", &asset);
            }
            _ => {}
        }
    }

    let _ = cmd_tx.send(BroadcastCmd::CleanUp(asset));
    Ok(())
}
