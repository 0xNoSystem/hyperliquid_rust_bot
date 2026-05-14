use super::CacheCmdIn;
use super::PriceData;
use crate::helper::{
    info_call_timeout, info_client_with_reconnect_timeout, info_client_with_timeout,
    info_unsubscribe_timeout,
};
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
    mpsc::{Receiver, Sender, channel, error::TrySendError},
    oneshot,
};
use tokio::time::{Duration, interval, timeout};
use tokio_util::sync::CancellationToken;

const BROADCAST_CMD_CHANNEL_SIZE: usize = 2048;
const SUBSCRIPTION_ATTEMPTS_MAX: u32 = 5;
const OI_UNSUB_BELOW: f64 = 100_000.0;
const ASSET_CONTEXT_REFRESH_SECS: u64 = 12 * 60 * 60;
const BROADCAST_INTERNAL_SEND_TIMEOUT_SECS: u64 = 5;

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
    subscribers: usize,
}

impl AssetFeed {
    fn is_idle(&self) -> bool {
        self.subscribers == 0
    }

    fn decrement_subscriber(&mut self) {
        self.subscribers = self.subscribers.saturating_sub(1);
    }
}

pub struct Broadcaster {
    url: BaseUrl,
    info_client: Arc<Mutex<InfoClient>>,
    cmd_tx: Sender<BroadcastCmd>,
    cmd_rx: Receiver<BroadcastCmd>,
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
    ) -> Result<(Self, Sender<BroadcastCmd>), Error> {
        let info_client = info_client_with_reconnect_timeout("broadcaster", url).await?;
        let universe = get_all_assets(&info_client).await?;
        let (cmd_tx, cmd_rx) = channel::<BroadcastCmd>(BROADCAST_CMD_CHANNEL_SIZE);
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
        let SubscribePayload { asset, reply } = sub_req;

        let meta = match self.universe.iter().find(|a| a.name == *asset) {
            Some(m) => m.clone(),
            None => {
                let _ = reply.send(Err(Error::AssetNotFound));
                return Ok(());
            }
        };

        if let Some(feed) = self.channels.get(&asset) {
            let rx = feed.tx.subscribe();
            if reply
                .send(Ok(SubscriptionReply {
                    px_receiver: rx,
                    meta,
                }))
                .is_ok()
                && let Some(feed) = self.channels.get_mut(&asset)
            {
                feed.subscribers = feed.subscribers.saturating_add(1);
            }
            return Ok(());
        }

        let rx = {
            let (tx, rx) = broadcast::channel::<PriceData>(256);
            if let Err(err) = self.cache_tx.try_send(CacheCmdIn::NewFeed {
                asset: Arc::clone(&asset),
                rx: tx.subscribe(),
            }) {
                let err = match err {
                    TrySendError::Full(_) => Error::Custom("CandleCache command queue full".into()),
                    TrySendError::Closed(_) => Error::Custom("CandleCache channel closed".into()),
                };
                let _ = reply.send(Err(err));
                return Ok(());
            }

            self.channels.insert(
                Arc::clone(&asset),
                AssetFeed {
                    tx: tx.clone(),
                    sub_id: None,
                    subscribers: 0,
                },
            );

            let info_client = self.info_client.clone();
            let cmd_tx = self.cmd_tx.clone();
            let asset = Arc::clone(&asset);
            let url = self.url;

            tokio::spawn(async move {
                let cleanup_tx = cmd_tx.clone();
                if let Err(e) =
                    spawn_hl_feed(info_client, cmd_tx, Arc::clone(&asset), tx, url).await
                {
                    log::error!("HL feed for {} exited with error: {:?}", &asset, e);
                    queue_cleanup(&cleanup_tx, asset);
                }
            });

            rx
        };

        if reply
            .send(Ok(SubscriptionReply {
                px_receiver: rx,
                meta,
            }))
            .is_ok()
        {
            if let Some(feed) = self.channels.get_mut(&asset) {
                feed.subscribers = 1;
            }
        } else {
            self.unsubscribe_from_feed(asset).await;
        }

        Ok(())
    }

    fn drop_cache_feed(&self, asset: Arc<str>) {
        match self.cache_tx.try_send(CacheCmdIn::DropFeed(asset.clone())) {
            Ok(()) => {}
            Err(TrySendError::Full(cmd)) => {
                let cache_tx = self.cache_tx.clone();
                let asset_name = Arc::clone(&asset);
                tokio::spawn(async move {
                    match timeout(
                        Duration::from_secs(BROADCAST_INTERNAL_SEND_TIMEOUT_SECS),
                        cache_tx.send(cmd),
                    )
                    .await
                    {
                        Ok(Ok(())) => {}
                        Ok(Err(_)) => {
                            log::warn!(
                                "candle cache channel closed before delayed feed drop for {}",
                                &asset_name
                            );
                        }
                        Err(_) => {
                            log::warn!("timed out queuing delayed feed drop for {}", &asset_name);
                        }
                    }
                });
                log::warn!("candle cache queue full while dropping feed for {}", &asset);
            }
            Err(TrySendError::Closed(_)) => {
                log::warn!(
                    "candle cache channel closed while dropping feed for {}",
                    &asset
                );
            }
        }
    }

    fn spawn_remote_unsubscribe(&self, asset: Arc<str>, sub_id: u32) {
        let client = self.info_client.clone();
        tokio::spawn(async move {
            let mut client = client.lock().await;
            if let Err(e) = info_unsubscribe_timeout("broadcaster", &mut client, sub_id).await {
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
            self.drop_cache_feed(Arc::clone(&asset));
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
            .map(AssetFeed::is_idle)
            .unwrap_or(true)
    }

    fn decrement_subscriber(&mut self, asset: &Arc<str>) {
        if let Some(feed) = self.channels.get_mut(asset) {
            feed.decrement_subscriber();
        }
    }

    fn should_unsubscribe_by_open_interest(
        asset_contexts: &FxMap<Arc<str>, f64>,
        asset: &Arc<str>,
    ) -> bool {
        match asset_contexts.get(asset) {
            Some(open_interest) => *open_interest < OI_UNSUB_BELOW,
            None => true,
        }
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
            if let Some(open_interest) = parse_open_interest(&asset.name, &context.open_interest) {
                asset_contexts.insert(Arc::from(asset.name), open_interest);
            }
        }
    }

    async fn refresh_asset_contexts(&mut self) -> Result<(), Error> {
        let info_client = info_client_with_timeout("broadcaster asset contexts", self.url).await?;
        let mut asset_contexts = FxMap::default();

        let (meta, contexts) = info_call_timeout(
            "broadcaster meta_and_asset_contexts",
            info_client.meta_and_asset_contexts(),
        )
        .await?;
        Self::update_asset_contexts_for_meta(&mut asset_contexts, meta, contexts);

        for dex in info_call_timeout("broadcaster perp_dexs", info_client.perp_dexs())
            .await?
            .into_iter()
            .flatten()
        {
            let label = format!("broadcaster meta_and_asset_contexts_for_dex {}", dex.name);
            let (meta, contexts) = info_call_timeout(
                &label,
                info_client.meta_and_asset_contexts_for_dex(dex.name.clone()),
            )
            .await?;
            Self::update_asset_contexts_for_meta(&mut asset_contexts, meta, contexts);
        }

        self.asset_contexts = asset_contexts;
        Ok(())
    }

    pub async fn start(&mut self, shutdown: CancellationToken) {
        if let Err(e) = self.refresh_asset_contexts().await {
            log::warn!("failed to refresh asset contexts on startup: {:?}", e);
        }

        let mut asset_context_refresh = interval(Duration::from_secs(ASSET_CONTEXT_REFRESH_SECS));
        // Consume the immediate first tick so the interval starts its 12h countdown.
        // Without this, the first select! iteration would double-refresh.
        asset_context_refresh.tick().await;

        loop {
            tokio::select! {
                _ = shutdown.cancelled() => {
                    break;
                }
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
                            self.decrement_subscriber(&asset);
                            if self.is_feed_idle(&asset)
                                && Self::should_unsubscribe_by_open_interest(
                                    &self.asset_contexts,
                                    &asset,
                                )
                            {
                                self.unsubscribe_from_feed(asset).await;
                            }
                        }
                        BroadcastCmd::SetSubId { asset, sub_id } => self.set_sub_id(&asset, sub_id),
                        BroadcastCmd::CleanUp(asset) => {
                            log::warn!("cleaning up dead feed for {}", &asset);
                            self.channels.remove(&asset);
                            self.pending_unsubscribes.remove(&asset);
                            self.drop_cache_feed(asset);
                        }
                    }
                }
            }
        }
    }
}

async fn spawn_hl_feed(
    info_client: Arc<Mutex<InfoClient>>,
    cmd_tx: Sender<BroadcastCmd>,
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

    if let Err(err) = match timeout(
        Duration::from_secs(BROADCAST_INTERNAL_SEND_TIMEOUT_SECS),
        cmd_tx.send(BroadcastCmd::SetSubId {
            asset: Arc::clone(&asset),
            sub_id,
        }),
    )
    .await
    {
        Ok(Ok(())) => Ok(()),
        Ok(Err(err)) => Err(Error::Custom(format!(
            "failed to queue sub id for {}: {}",
            &asset, err
        ))),
        Err(_) => Err(Error::Custom(format!(
            "timed out queuing sub id for {}",
            &asset
        ))),
    } {
        let mut client = info_client.lock().await;
        if let Err(unsub_err) = info_unsubscribe_timeout("broadcaster", &mut client, sub_id).await {
            log::warn!(
                "failed to clean up {} subscription {} after SetSubId failure: {:?}",
                &asset,
                sub_id,
                unsub_err
            );
        }
        return Err(err);
    }

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
                            let fetch_asset = Arc::clone(&asset);
                            let fetch_tx = tx.clone();

                            tokio::spawn(async move {
                                let fetch_client = info_client_with_timeout(
                                    "broadcaster reconnect gap fetch",
                                    url,
                                )
                                .await;

                                if let Ok(client) = fetch_client {
                                    match candles_snapshot(
                                        &client,
                                        &fetch_asset,
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
                                                &fetch_asset
                                            );
                                            let _ = fetch_tx.send(PriceData::Bulk(missed));
                                        }
                                        Ok(_) => {}
                                        Err(e) => {
                                            log::warn!(
                                                "failed to fetch missed window for {}: {:?}",
                                                &fetch_asset,
                                                e
                                            );
                                        }
                                    }
                                } else if let Err(e) = fetch_client {
                                    log::warn!(
                                        "failed to create recovery InfoClient for {}: {:?}",
                                        &fetch_asset,
                                        e
                                    );
                                }
                            });
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

    queue_cleanup(&cmd_tx, asset);
    Ok(())
}

fn queue_cleanup(cmd_tx: &Sender<BroadcastCmd>, asset: Arc<str>) {
    match cmd_tx.try_send(BroadcastCmd::CleanUp(Arc::clone(&asset))) {
        Ok(()) => {}
        Err(TrySendError::Full(cmd)) => {
            let cleanup_tx = cmd_tx.clone();
            let asset_name = Arc::clone(&asset);
            tokio::spawn(async move {
                match timeout(
                    Duration::from_secs(BROADCAST_INTERNAL_SEND_TIMEOUT_SECS),
                    cleanup_tx.send(cmd),
                )
                .await
                {
                    Ok(Ok(())) => {}
                    Ok(Err(_)) => {
                        log::warn!(
                            "broadcaster command channel closed before delayed cleanup for {}",
                            &asset_name
                        );
                    }
                    Err(_) => {
                        log::warn!("timed out queuing delayed cleanup for {}", &asset_name);
                    }
                }
            });
            log::warn!(
                "broadcaster command queue full while cleaning up {}",
                &asset
            );
        }
        Err(TrySendError::Closed(_)) => {
            log::warn!("failed to queue cleanup for {}: channel closed", &asset);
        }
    }
}

fn parse_open_interest(asset: &str, raw: &str) -> Option<f64> {
    match raw.parse::<f64>() {
        Ok(open_interest) if open_interest.is_finite() && open_interest >= 0.0 => {
            Some(open_interest)
        }
        Ok(open_interest) => {
            log::warn!("invalid open interest for {asset}: {open_interest}");
            None
        }
        Err(e) => {
            log::warn!("failed to parse open interest for {asset}: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_feed_idle_ignores_internal_cache_receiver() {
        let (tx, cache_rx) = broadcast::channel::<PriceData>(16);
        let mut feed = AssetFeed {
            tx,
            sub_id: None,
            subscribers: 1,
        };

        assert_eq!(feed.tx.receiver_count(), 1);
        assert!(!feed.is_idle());

        feed.decrement_subscriber();

        assert_eq!(feed.tx.receiver_count(), 1);
        assert!(feed.is_idle());

        drop(cache_rx);
    }

    #[test]
    fn unknown_open_interest_unsubscribes_idle_feed() {
        let asset = Arc::<str>::from("UNKNOWN");
        let contexts = FxMap::default();

        assert!(Broadcaster::should_unsubscribe_by_open_interest(
            &contexts, &asset
        ));
    }

    #[test]
    fn high_open_interest_keeps_idle_feed_warm() {
        let asset = Arc::<str>::from("BTC");
        let mut contexts = FxMap::default();
        contexts.insert(Arc::clone(&asset), OI_UNSUB_BELOW + 1.0);

        assert!(!Broadcaster::should_unsubscribe_by_open_interest(
            &contexts, &asset
        ));
    }

    #[test]
    fn low_open_interest_unsubscribes_idle_feed() {
        let asset = Arc::<str>::from("MEME");
        let mut contexts = FxMap::default();
        contexts.insert(Arc::clone(&asset), OI_UNSUB_BELOW - 1.0);

        assert!(Broadcaster::should_unsubscribe_by_open_interest(
            &contexts, &asset
        ));
    }

    #[test]
    fn parse_open_interest_rejects_non_finite_and_negative_values() {
        assert_eq!(parse_open_interest("BTC", "123.45"), Some(123.45));
        assert_eq!(parse_open_interest("BTC", "NaN"), None);
        assert_eq!(parse_open_interest("BTC", "inf"), None);
        assert_eq!(parse_open_interest("BTC", "-1"), None);
        assert_eq!(parse_open_interest("BTC", "not-a-number"), None);
    }
}
