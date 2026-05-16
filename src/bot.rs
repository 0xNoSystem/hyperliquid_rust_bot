use crate::{
    AddMarketInfo, BackendStatus, EngineView, ExecEvent, HLTradeInfo, Market, MarketCommand,
    MarketInfo, MarketState, MarketUpdate, TradeFillInfo, TradeInfo, UpdateFrontend, UserSession,
    Wallet,
};

use crate::backend::app_state::{StrategyCache, WsConnections, broadcast_to_user};
use crate::broadcast::{
    BroadcastCmd, CacheCmdIn, PriceAsset, PriceData, SubReply, SubscribePayload,
    UserEventRelayHandle,
};
use crate::stream::AccountEvent;
use hyperliquid_rust_sdk::{
    AssetMeta, AssetPosition, BaseUrl, Error, InfoClient, LedgerUpdate, LedgerUpdateData, Message,
    Subscription, UserData,
};
use log::warn;
use rhai::Engine;
use rustc_hash::FxHasher;
use serde::Deserialize;
use sqlx::PgPool;
use std::collections::{HashMap, HashSet};
use std::hash::BuildHasherDefault;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc::{Receiver, Sender, channel, error::TrySendError, unbounded_channel};
use tokio::sync::{Mutex, oneshot};
use tokio::task::JoinHandle;
use tokio::time::{Duration, sleep, timeout};
use tokio_util::sync::CancellationToken;

use crate::helper::*;
use crate::margin::{AssetMargin, MarginBook};
use crate::metrics;
use crate::{DEFAULT_BUILDER_ADDRESS, DEFAULT_BUILDER_FEE};

pub type Session = Arc<Mutex<HashMap<String, MarketState, BuildHasherDefault<FxHasher>>>>;
const MARGIN_SYNC_MIN_INTERVAL_SECS: u64 = 5;
const PRICE_ROUTER_CHANNEL_SIZE: usize = 1024;
const MARKET_PRICE_CHANNEL_SIZE: usize = 512;
const USER_EVENT_QUEUE_SIZE: usize = 512;
const MARKET_UPDATE_CHANNEL_SIZE: usize = 2048;
const TRADE_PERSIST_QUEUE_SIZE: usize = 1024;
const BROADCAST_SUBSCRIBE_TIMEOUT_SECS: u64 = 5;
const MARKET_COMMAND_SEND_TIMEOUT_SECS: u64 = 5;
const BOT_EVENT_SEND_TIMEOUT_SECS: u64 = 5;
const MARGIN_SYNC_FALLBACK_SECS: u64 = 30;
const MARGIN_SYNC_STAGGER_MAX_SECS: u64 = 30;
const BOT_DB_QUERY_TIMEOUT_SECS: u64 = 10;
const TRADE_PERSIST_TIMEOUT_SECS: u64 = 10;
const EMPTY_MARKET_IDLE_TIMEOUT_SECS: u64 = 4 * 60 * 60;
const EMPTY_MARKET_IDLE_CHECK_SECS: u64 = 60;
const MARKET_TASK_JOIN_TIMEOUT_SECS: u64 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MarketCommandSendResult {
    Sent,
    Closed,
    TimedOut,
    Missing,
}

async fn send_market_command_with_timeout(
    asset: &str,
    tx: &Sender<MarketCommand>,
    cmd: MarketCommand,
    label: &'static str,
    wait: Duration,
) -> MarketCommandSendResult {
    match tx.try_send(cmd) {
        Ok(()) => MarketCommandSendResult::Sent,
        Err(TrySendError::Full(cmd)) => match timeout(wait, tx.send(cmd)).await {
            Ok(Ok(())) => MarketCommandSendResult::Sent,
            Ok(Err(_)) => {
                warn!("failed to send {label} command to {asset}: market channel closed");
                MarketCommandSendResult::Closed
            }
            Err(_) => {
                warn!("timed out sending {label} command to {asset}: market command queue full");
                MarketCommandSendResult::TimedOut
            }
        },
        Err(TrySendError::Closed(_)) => {
            warn!("failed to send {label} command to {asset}: market channel closed");
            MarketCommandSendResult::Closed
        }
    }
}

async fn send_market_command(
    asset: &str,
    tx: &Sender<MarketCommand>,
    cmd: MarketCommand,
    label: &'static str,
) -> MarketCommandSendResult {
    send_market_command_with_timeout(
        asset,
        tx,
        cmd,
        label,
        Duration::from_secs(MARKET_COMMAND_SEND_TIMEOUT_SECS),
    )
    .await
}

async fn release_market_margin(asset: &str, margin_book: &Arc<Mutex<MarginBook>>) -> f64 {
    let stale_total = {
        let mut book = margin_book.lock().await;
        book.remove(asset);
        book.free()
    };

    match MarginBook::sync_total_if_stale_shared(margin_book, Duration::ZERO).await {
        Ok(total) => total,
        Err(err) => {
            warn!("failed to sync margin after removing {asset}; using stale local total: {err}");
            stale_total
        }
    }
}

async fn queue_bot_event(tx: &Sender<BotEvent>, event: BotEvent, label: &'static str) {
    match tx.try_send(event) {
        Ok(()) => {}
        Err(TrySendError::Full(event)) => {
            warn!("bot event queue full while queuing {label}");
            match timeout(
                Duration::from_secs(BOT_EVENT_SEND_TIMEOUT_SECS),
                tx.send(event),
            )
            .await
            {
                Ok(Ok(())) => {}
                Ok(Err(_)) => warn!("bot event channel closed before delayed {label}"),
                Err(_) => warn!("timed out queuing delayed bot event {label}"),
            }
        }
        Err(TrySendError::Closed(_)) => {
            warn!("bot event channel closed while queuing {label}");
        }
    }
}

fn margin_sync_fallback_delay(pubkey: &str) -> Duration {
    let stagger = pubkey.bytes().fold(0_u64, |acc, byte| {
        acc.wrapping_mul(31).wrapping_add(byte as u64)
    }) % MARGIN_SYNC_STAGGER_MAX_SECS;
    Duration::from_secs(MARGIN_SYNC_FALLBACK_SECS + stagger)
}

struct TradePersistence {
    asset: String,
    trade: TradeInfo,
}

fn queue_trade_persistence(
    tx: &Sender<TradePersistence>,
    item: TradePersistence,
    queue_full: &mut bool,
    dropped: &mut u64,
) {
    match tx.try_send(item) {
        Ok(()) => {
            if *queue_full {
                log::info!("trade persistence queue recovered after dropping {dropped} records");
                *queue_full = false;
                *dropped = 0;
            }
        }
        Err(TrySendError::Full(_)) => {
            metrics::inc_trade_persistence_dropped();
            *dropped = dropped.saturating_add(1);
            if !*queue_full {
                warn!("trade persistence queue full; dropping completed trade records");
                *queue_full = true;
            }
        }
        Err(TrySendError::Closed(_)) => {
            warn!("trade persistence queue closed; dropping completed trade record");
        }
    }
}

fn parse_user_funding(raw: &str) -> Result<f64, Error> {
    let funding = raw
        .parse::<f64>()
        .map_err(|err| Error::GenericParse(format!("failed to parse user funding: {err}")))?;

    if !funding.is_finite() {
        return Err(Error::GenericParse(
            "user funding was not finite".to_string(),
        ));
    }

    Ok(funding)
}

fn refresh_empty_market_idle(markets_empty: bool, idle_since: &mut Option<Instant>, now: Instant) {
    if markets_empty {
        idle_since.get_or_insert(now);
    } else {
        *idle_since = None;
    }
}

fn empty_market_idle_expired(
    markets_empty: bool,
    idle_since: &mut Option<Instant>,
    now: Instant,
    timeout: Duration,
) -> bool {
    refresh_empty_market_idle(markets_empty, idle_since, now);
    idle_since.is_some_and(|since| now.saturating_duration_since(since) >= timeout)
}

fn spawn_trade_persistence_worker(
    pool: PgPool,
    pubkey: String,
    mut rx: Receiver<TradePersistence>,
) {
    tokio::spawn(async move {
        while let Some(item) = rx.recv().await {
            persist_trade(&pool, &pubkey, item).await;
        }
    });
}

async fn persist_trade(pool: &PgPool, pubkey: &str, item: TradePersistence) {
    let side_str = format!("{:?}", item.trade.side);
    let open_type = format!("{:?}", item.trade.open.fill_type);
    let close_type = format!("{:?}", item.trade.close.fill_type);

    let query = sqlx::query(
        "INSERT INTO trades (pubkey, market, side, size, pnl, total_pnl, fees, funding, open_time, open_price, open_type, close_time, close_price, close_type, strategy) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)",
    )
    .bind(pubkey)
    .bind(&item.asset)
    .bind(&side_str)
    .bind(item.trade.size)
    .bind(item.trade.pnl)
    .bind(item.trade.total_pnl)
    .bind(item.trade.fees)
    .bind(item.trade.funding)
    .bind(item.trade.open.time as i64)
    .bind(item.trade.open.price)
    .bind(&open_type)
    .bind(item.trade.close.time as i64)
    .bind(item.trade.close.price)
    .bind(&close_type)
    .bind(&item.trade.strategy);

    match timeout(
        Duration::from_secs(TRADE_PERSIST_TIMEOUT_SECS),
        query.execute(pool),
    )
    .await
    {
        Ok(Ok(_)) => {}
        Ok(Err(e)) => log::warn!("Failed to persist trade for {}: {:?}", item.asset, e),
        Err(_) => log::warn!("Timed out persisting trade for {}", item.asset),
    }
}

pub struct Bot {
    info_client: InfoClient,
    wallet: Arc<Wallet>,
    markets: HashMap<String, Sender<MarketCommand>, BuildHasherDefault<FxHasher>>,
    market_price_routes: HashMap<String, Sender<PriceAsset>, BuildHasherDefault<FxHasher>>,
    saturated_market_price_routes: HashSet<String, BuildHasherDefault<FxHasher>>,
    market_required_assets: HashMap<String, HashSet<Arc<str>>, BuildHasherDefault<FxHasher>>,
    asset_consumers: HashMap<Arc<str>, HashSet<String>, BuildHasherDefault<FxHasher>>,
    asset_feeds: HashMap<Arc<str>, BotAssetFeed, BuildHasherDefault<FxHasher>>,
    broadcast_tx: Sender<BroadcastCmd>,
    candle_rx: Sender<CacheCmdIn>,
    user_event_relay: Option<UserEventRelayHandle>,
    _fees: (f64, f64),
    _bot_tx: Sender<BotEvent>,
    bot_rv: Receiver<BotEvent>,
    market_handles: HashMap<String, JoinHandle<()>, BuildHasherDefault<FxHasher>>,
    price_router_rv: Option<Receiver<PriceAsset>>,
    price_router_tx: Sender<PriceAsset>,
    update_rv: Option<Receiver<MarketUpdate>>,
    update_tx: Sender<MarketUpdate>,
    ws_connections: Option<WsConnections>,
    pubkey: Option<String>,
    pool: Option<PgPool>,
    rhai_engine: Option<Arc<Engine>>,
    strategy_cache: Option<StrategyCache>,
    chain_open_positions: Vec<AssetPosition>,
    key_valid: bool,
    builder_approved: bool,
}

struct BotAssetFeed {
    meta: AssetMeta,
    handle: JoinHandle<()>,
}

enum UserEventMessage {
    QuickNode(AccountEvent),
    Sdk(Message),
}

impl Bot {
    pub async fn new(
        wallet: Wallet,
        broadcast_tx: Sender<BroadcastCmd>,
        candle_rx: Sender<CacheCmdIn>,
        user_event_relay: Option<UserEventRelayHandle>,
    ) -> Result<(Self, Sender<BotEvent>), Error> {
        let info_client = info_client_with_reconnect_timeout("bot", wallet.url).await?;
        let fees = wallet.get_user_fees().await?;

        let (bot_tx, bot_rv) = channel::<BotEvent>(64);
        let (price_router_tx, price_router_rv) = channel::<PriceAsset>(PRICE_ROUTER_CHANNEL_SIZE);
        let (update_tx, update_rv) = channel::<MarketUpdate>(MARKET_UPDATE_CHANNEL_SIZE);

        Ok((
            Self {
                info_client,
                wallet: wallet.into(),
                markets: HashMap::default(),
                market_price_routes: HashMap::default(),
                saturated_market_price_routes: HashSet::default(),
                market_required_assets: HashMap::default(),
                asset_consumers: HashMap::default(),
                asset_feeds: HashMap::default(),
                broadcast_tx,
                candle_rx,
                user_event_relay,
                _fees: fees,
                _bot_tx: bot_tx.clone(),
                bot_rv,
                market_handles: HashMap::default(),
                price_router_rv: Some(price_router_rv),
                price_router_tx,
                update_rv: Some(update_rv),
                update_tx,
                ws_connections: None,
                pubkey: None,
                pool: None,
                rhai_engine: None,
                strategy_cache: None,
                chain_open_positions: Vec::new(),
                key_valid: true,
                builder_approved: true,
            },
            bot_tx,
        ))
    }

    /// Helper: broadcast a message to all connected devices for this bot's user.
    async fn send_to_frontend(&self, msg: UpdateFrontend) {
        if let (Some(conns), Some(pubkey)) = (&self.ws_connections, &self.pubkey) {
            broadcast_to_user(conns, pubkey, msg).await;
        }
    }

    async fn refresh_builder_approval_status(&mut self, pubkey: &str) {
        let user = match address(pubkey) {
            Ok(user) => user,
            Err(err) => {
                log::warn!("[bot] failed to parse user address for builder fee check: {err}");
                return;
            }
        };
        let builder = match address(DEFAULT_BUILDER_ADDRESS) {
            Ok(builder) => builder,
            Err(err) => {
                log::error!("[bot] failed to parse configured builder address: {err}");
                return;
            }
        };

        match info_call_timeout(
            "builder fee approval",
            self.info_client.max_builder_fee(user, builder),
        )
        .await
        {
            Ok(max_fee) => {
                self.builder_approved = max_fee.0 >= DEFAULT_BUILDER_FEE;
                self.send_to_frontend(UpdateFrontend::NeedsBuilderApproval(!self.builder_approved))
                    .await;
                log::info!(
                    "[bot] builder fee approval checked user={} max_fee={} required={} approved={}",
                    pubkey,
                    max_fee.0,
                    DEFAULT_BUILDER_FEE,
                    self.builder_approved
                );
            }
            Err(err) => {
                log::warn!("[bot] failed to check builder fee approval for user={pubkey}: {err}");
            }
        }
    }

    pub(crate) async fn shutdown_unused(mut self) {
        let _ = info_shutdown_ws_timeout("bot", &mut self.info_client).await;
    }

    async fn ensure_asset_feed(&mut self, asset: Arc<str>) -> Result<AssetMeta, Error> {
        if let Some(feed) = self.asset_feeds.get(&asset) {
            return Ok(feed.meta.clone());
        }

        let (one_tx, one_rx) = oneshot::channel::<SubReply>();
        let sub_request = SubscribePayload {
            asset: Arc::clone(&asset),
            reply: one_tx,
        };

        match self
            .broadcast_tx
            .try_send(BroadcastCmd::Subscribe(sub_request))
        {
            Ok(()) => {}
            Err(TrySendError::Full(cmd)) => {
                timeout(
                    Duration::from_secs(BROADCAST_SUBSCRIBE_TIMEOUT_SECS),
                    self.broadcast_tx.send(cmd),
                )
                .await
                .map_err(|_| Error::Custom("timed out queuing broadcast subscription".into()))?
                .map_err(|e| Error::Custom(format!("broadcast channel closed: {}", e)))?;
            }
            Err(TrySendError::Closed(_)) => {
                return Err(Error::Custom("broadcast channel closed".into()));
            }
        }

        let sub_info = timeout(
            Duration::from_secs(BROADCAST_SUBSCRIBE_TIMEOUT_SECS),
            one_rx,
        )
        .await
        .map_err(|_| Error::Custom("timed out waiting for subscription reply".to_string()))?
        .map_err(|_| Error::Custom("subscription reply dropped".to_string()))??;

        let meta = sub_info.meta.clone();
        let mut px_receiver = sub_info.px_receiver;
        let price_router_tx = self.price_router_tx.clone();
        let bot_tx = self._bot_tx.clone();
        let asset_key = Arc::clone(&asset);

        let handle = tokio::spawn(async move {
            let mut queue_full = false;
            let mut dropped = 0_u64;
            loop {
                match px_receiver.recv().await {
                    Ok(data) => match price_router_tx.try_send((Arc::clone(&asset_key), data)) {
                        Ok(()) => {
                            if queue_full {
                                log::info!(
                                    "{} bot price router recovered after dropping {} updates",
                                    &asset_key,
                                    dropped
                                );
                                queue_full = false;
                                dropped = 0;
                            }
                        }
                        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                            metrics::inc_price_router_dropped();
                            dropped = dropped.saturating_add(1);
                            if !queue_full {
                                log::warn!(
                                    "{} bot price router full; dropping live price updates",
                                    &asset_key
                                );
                                queue_full = true;
                            }
                        }
                        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => break,
                    },
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        log::warn!("{} bot feed lagged by {} messages", &asset_key, n);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        queue_bot_event(
                            &bot_tx,
                            BotEvent::AssetFeedDied((*asset_key).to_string()),
                            "AssetFeedDied",
                        )
                        .await;
                        break;
                    }
                }
            }
        });

        self.asset_feeds.insert(
            asset,
            BotAssetFeed {
                meta: meta.clone(),
                handle,
            },
        );
        Ok(meta)
    }

    async fn drop_asset_feed(&mut self, asset: &Arc<str>) -> bool {
        if let Some(feed) = self.asset_feeds.remove(asset) {
            feed.handle.abort();
            let _ = feed.handle.await;
            return true;
        }
        false
    }

    async fn unsubscribe_asset_if_idle(&mut self, asset: Arc<str>) {
        if self
            .asset_consumers
            .get(&asset)
            .is_some_and(|consumers| !consumers.is_empty())
        {
            return;
        }

        self.asset_consumers.remove(&asset);

        if self.drop_asset_feed(&asset).await {
            self.queue_broadcast_unsubscribe(asset).await;
        }
    }

    async fn drop_all_asset_feeds(&mut self) {
        let assets = self.asset_feeds.keys().cloned().collect::<Vec<_>>();
        for asset in assets {
            if self.drop_asset_feed(&asset).await {
                self.queue_broadcast_unsubscribe(asset).await;
            }
        }

        self.asset_consumers.clear();
        self.market_required_assets.clear();
        self.market_price_routes.clear();
        self.saturated_market_price_routes.clear();
    }

    async fn join_market_task(&mut self, asset: &str) {
        let Some(mut handle) = self.market_handles.remove(asset) else {
            return;
        };

        match timeout(
            Duration::from_secs(MARKET_TASK_JOIN_TIMEOUT_SECS),
            &mut handle,
        )
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                warn!("market task for {asset} failed while joining: {err}");
            }
            Err(_) => {
                warn!("timed out joining market task for {asset}; aborting task");
                handle.abort();
                let _ = handle.await;
            }
        }
    }

    async fn join_all_market_tasks(&mut self) {
        self.markets.clear();
        let assets = self.market_handles.keys().cloned().collect::<Vec<_>>();
        for asset in assets {
            self.join_market_task(&asset).await;
        }
    }

    async fn queue_broadcast_unsubscribe(&self, asset: Arc<str>) {
        match self
            .broadcast_tx
            .try_send(BroadcastCmd::Unsubscribe(Arc::clone(&asset)))
        {
            Ok(()) => {}
            Err(TrySendError::Full(cmd)) => {
                warn!("broadcast queue full while unsubscribing {}", &asset);
                match timeout(
                    Duration::from_secs(BROADCAST_SUBSCRIBE_TIMEOUT_SECS),
                    self.broadcast_tx.send(cmd),
                )
                .await
                {
                    Ok(Ok(())) => {}
                    Ok(Err(_)) => {
                        warn!(
                            "broadcast channel closed before delayed unsubscribe for {}",
                            &asset
                        );
                    }
                    Err(_) => {
                        warn!(
                            "timed out queuing delayed broadcaster unsubscribe for {}",
                            &asset
                        );
                    }
                }
            }
            Err(TrySendError::Closed(_)) => {
                warn!("failed to unsubscribe {} from broadcaster", &asset);
            }
        }
    }

    fn route_price(&mut self, asset: Arc<str>, data: PriceData) {
        let Some(markets) = self.asset_consumers.get(&asset) else {
            return;
        };
        let markets = markets.iter().cloned().collect::<Vec<_>>();

        for market in markets {
            if let Some(tx) = self.market_price_routes.get(&market) {
                match tx.try_send((Arc::clone(&asset), data.clone())) {
                    Ok(()) => {
                        if self.saturated_market_price_routes.remove(&market) {
                            log::info!("price route recovered for market {market}");
                        }
                    }
                    Err(TrySendError::Full(_)) => {
                        metrics::inc_market_price_route_dropped();
                        if self.saturated_market_price_routes.insert(market.clone()) {
                            log::warn!(
                                "price route full for market {market}; dropping live price updates"
                            );
                        }
                    }
                    Err(TrySendError::Closed(_)) => {
                        if self.saturated_market_price_routes.insert(market.clone()) {
                            log::warn!("price route closed for market {market}; dropping updates");
                        }
                    }
                }
            }
        }
    }

    async fn sync_market_feeds(
        &mut self,
        market: &str,
        required_assets: HashSet<Arc<str>>,
    ) -> Result<(), Error> {
        if !self.market_price_routes.contains_key(market) {
            return Err(Error::Custom(format!(
                "market route missing for {}",
                market
            )));
        }

        let current_assets = self
            .market_required_assets
            .get(market)
            .cloned()
            .unwrap_or_default();

        let to_add: Vec<_> = required_assets
            .difference(&current_assets)
            .cloned()
            .collect();
        let to_remove: Vec<_> = current_assets
            .difference(&required_assets)
            .cloned()
            .collect();

        let mut newly_created_feeds = Vec::new();
        for asset in &to_add {
            let had_feed = self.asset_feeds.contains_key(asset);
            if let Err(e) = self.ensure_asset_feed(Arc::clone(asset)).await {
                for created in newly_created_feeds {
                    self.unsubscribe_asset_if_idle(created).await;
                }
                return Err(e);
            }
            if !had_feed {
                newly_created_feeds.push(Arc::clone(asset));
            }
        }

        for asset in &to_add {
            self.asset_consumers
                .entry(Arc::clone(asset))
                .or_default()
                .insert(market.to_string());
        }

        for asset in to_remove {
            let should_drop = if let Some(consumers) = self.asset_consumers.get_mut(&asset) {
                consumers.remove(market);
                consumers.is_empty()
            } else {
                true
            };

            if should_drop {
                self.unsubscribe_asset_if_idle(asset).await;
            }
        }

        self.market_required_assets
            .insert(market.to_string(), required_assets);

        Ok(())
    }

    async fn clear_market_feed_state(&mut self, market: &str) {
        self.market_price_routes.remove(market);
        self.saturated_market_price_routes.remove(market);
        let mut required_assets = self
            .market_required_assets
            .remove(market)
            .unwrap_or_default();
        required_assets.insert(Arc::<str>::from(market));

        for asset in required_assets {
            let should_drop = if let Some(consumers) = self.asset_consumers.get_mut(&asset) {
                consumers.remove(market);
                consumers.is_empty()
            } else {
                true
            };

            if should_drop {
                self.unsubscribe_asset_if_idle(asset).await;
            }
        }
    }

    pub async fn add_market(
        &mut self,
        info: AddMarketInfo,
        margin_book: &Arc<Mutex<MarginBook>>,
    ) -> Result<(), Error> {
        let AddMarketInfo {
            asset,
            margin_alloc,
            lev,
            strategy_id,
            config,
        } = info;

        if lev == 0 {
            return Err(Error::Custom(
                "leverage must be greater than zero".to_string(),
            ));
        }

        // Resolve strategy → compiled strategy + indicators + name
        let rhai_engine = self
            .rhai_engine
            .as_ref()
            .ok_or_else(|| Error::Custom("rhai engine not initialized".to_string()))?
            .clone();

        let (compiled, strat_indicators, strategy_name) = if let Some(sid) = strategy_id {
            let cache = self
                .strategy_cache
                .as_ref()
                .ok_or_else(|| Error::Custom("strategy cache not initialized".to_string()))?;

            // Try cache first
            let cached = {
                let guard = cache.read().await;
                guard.get(&sid).cloned()
            };

            if let Some(entry) = cached {
                (entry.compiled, entry.indicators, entry.name)
            } else {
                // Cache miss — fetch from DB, compile, and cache
                let pool = self
                    .pool
                    .as_ref()
                    .ok_or_else(|| Error::Custom("DB pool not initialized".to_string()))?;

                let row = timeout(
                    Duration::from_secs(BOT_DB_QUERY_TIMEOUT_SECS),
                    sqlx::query_as::<_, crate::backend::db::StrategyRow>(
                        "SELECT * FROM strategies WHERE id = $1",
                    )
                    .bind(sid)
                    .fetch_optional(pool),
                )
                .await
                .map_err(|_| Error::Custom("DB query timed out fetching strategy".to_string()))?
                .map_err(|e| Error::Custom(format!("DB error fetching strategy: {e}")))?
                .ok_or_else(|| Error::Custom(format!("strategy {sid} not found")))?;

                let state_decls: Option<crate::backend::scripting::StateDeclarations> = match row
                    .state_declarations
                    .as_ref()
                    .map(|v| serde_json::from_value(v.clone()))
                {
                    Some(Ok(decls)) => Some(decls),
                    Some(Err(e)) => {
                        return Err(Error::Custom(format!(
                            "strategy {sid} has invalid state declarations: {e}"
                        )));
                    }
                    None => None,
                };

                let compiled = crate::backend::scripting::compile_strategy(
                    &rhai_engine,
                    &row.on_idle,
                    &row.on_open,
                    &row.on_busy,
                    state_decls.as_ref(),
                )
                .map_err(|e| Error::Custom(format!("strategy {sid} failed to compile: {e}")))?;

                let indicators: Vec<crate::IndexId> = serde_json::from_value(row.indicators)
                    .map_err(|e| {
                        Error::Custom(format!("strategy {sid} has invalid indicators: {e}"))
                    })?;

                // Cache for next time
                {
                    let mut guard = cache.write().await;
                    guard.insert(
                        sid,
                        crate::backend::app_state::CachedStrategy {
                            compiled: compiled.clone(),
                            indicators: indicators.clone(),
                            state_declarations: state_decls.clone(),
                            name: row.name.clone(),
                        },
                    );
                }

                (compiled, indicators, row.name)
            }
        } else {
            // View-only mode: no trading, just stream price + indicators
            let noop = crate::backend::scripting::CompiledStrategy::noop(&rhai_engine);
            (noop, vec![], "View Only".to_string())
        };

        let asset = asset.trim().to_string();

        if self.markets.contains_key(&asset) {
            self.send_to_frontend(UpdateFrontend::UserError(format!(
                "{} market is already added.",
                &asset
            )))
            .await;
            return Ok(());
        }

        self.chain_open_positions = MarginBook::sync_shared(margin_book).await?;
        if self
            .chain_open_positions
            .iter()
            .any(|p| p.position.coin == asset)
        {
            self.send_to_frontend(UpdateFrontend::UserError(format!(
                "Cannot add a market with open on-chain position({})",
                &asset
            )))
            .await;
            return Ok(());
        }

        let mut book = margin_book.lock().await;
        let margin = book.allocate_from_current(asset.clone(), margin_alloc)?;
        drop(book);

        self.send_to_frontend(UpdateFrontend::PreconfirmMarket(asset.clone()))
            .await;

        let market_asset = Arc::<str>::from(asset.as_str());
        let had_feed = self.asset_feeds.contains_key(&market_asset);
        let meta = self.ensure_asset_feed(Arc::clone(&market_asset)).await?;
        let (price_tx, price_rx) = channel::<PriceAsset>(MARKET_PRICE_CHANNEL_SIZE);

        let market_result = Market::new(
            self.wallet.clone(),
            self.update_tx.clone(),
            self._bot_tx.clone(),
            self.candle_rx.clone(),
            price_rx,
            meta,
            margin,
            lev,
            compiled,
            strat_indicators,
            strategy_name,
            config,
        )
        .await;
        let (market, market_tx) = match market_result {
            Ok(result) => result,
            Err(e) => {
                if !had_feed {
                    self.unsubscribe_asset_if_idle(market_asset).await;
                }
                return Err(e);
            }
        };

        self.markets.insert(asset.clone(), market_tx);
        self.market_price_routes.insert(asset.clone(), price_tx);
        self.market_required_assets
            .insert(asset.clone(), HashSet::default());
        let api_key_valid = self.key_valid;
        let trading_enabled = api_key_valid && self.builder_approved;
        if !api_key_valid {
            self.send_to_frontend(UpdateFrontend::NeedsApiKey(true))
                .await;
        }
        if !self.builder_approved {
            self.send_to_frontend(UpdateFrontend::NeedsBuilderApproval(true))
                .await;
        }

        let ws_conns = self.ws_connections.clone();
        let bot_pubkey = self.pubkey.clone();
        let remove_market_tx = self._bot_tx.clone();
        let task_asset = asset.clone();

        let handle = tokio::spawn(async move {
            if let Err(e) = market.start(trading_enabled, api_key_valid).await {
                if let (Some(conns), Some(pk)) = (&ws_conns, &bot_pubkey) {
                    broadcast_to_user(conns, pk, UpdateFrontend::CancelMarket(task_asset.clone()))
                        .await;

                    broadcast_to_user(
                        conns,
                        pk,
                        UpdateFrontend::UserError(format!(
                            "Market {} exited with error:\n {:?}",
                            &task_asset, e
                        )),
                    )
                    .await;
                }

                queue_bot_event(
                    &remove_market_tx,
                    BotEvent::RemoveMarket(task_asset.clone()),
                    "RemoveMarket",
                )
                .await;

                if let Error::AuthError(s) = e {
                    queue_bot_event(&remove_market_tx, BotEvent::AuthFailed(s), "RemoveMarket")
                        .await;
                }
            }
        });
        self.market_handles.insert(asset, handle);

        Ok(())
    }

    pub async fn remove_market(
        &mut self,
        asset: &str,
        margin_book: &Arc<Mutex<MarginBook>>,
    ) -> Result<(), Error> {
        let asset = asset.trim().to_string();

        if !self.markets.contains_key(&asset) {
            return Ok(());
        }

        if let Some(tx) = self.markets.get(&asset).cloned() {
            match send_market_command(&asset, &tx, MarketCommand::Close, "Close").await {
                MarketCommandSendResult::Sent
                | MarketCommandSendResult::Closed
                | MarketCommandSendResult::Missing => {
                    self.markets.remove(&asset);
                    let total = release_market_margin(&asset, margin_book).await;
                    self.send_to_frontend(UpdateFrontend::UpdateTotalMargin(total))
                        .await;
                    self.clear_market_feed_state(&asset).await;
                    self.join_market_task(&asset).await;
                }
                MarketCommandSendResult::TimedOut => {
                    return Err(Error::Custom(format!(
                        "timed out closing {asset}: market command queue full"
                    )));
                }
            }
        }

        Ok(())
    }
    pub async fn pause_all(&self) -> Vec<String> {
        let mut paused = Vec::new();
        for (asset, tx) in &self.markets {
            if send_market_command(asset, tx, MarketCommand::Pause, "Pause").await
                == MarketCommandSendResult::Sent
            {
                paused.push(asset.clone());
            }
        }
        paused
    }

    pub async fn resume_all(&self) -> Vec<String> {
        let mut resumed = Vec::new();
        for (asset, tx) in &self.markets {
            if send_market_command(asset, tx, MarketCommand::Resume, "Resume").await
                == MarketCommandSendResult::Sent
            {
                resumed.push(asset.clone());
            }
        }
        resumed
    }

    pub async fn close_all(&mut self) {
        let markets = std::mem::take(&mut self.markets);
        let mut closed_assets = Vec::new();

        for (asset, tx) in markets {
            match send_market_command(&asset, &tx, MarketCommand::Close, "Close").await {
                MarketCommandSendResult::Sent
                | MarketCommandSendResult::Closed
                | MarketCommandSendResult::Missing => {
                    self.clear_market_feed_state(&asset).await;
                    closed_assets.push(asset);
                }
                MarketCommandSendResult::TimedOut => {
                    self.markets.insert(asset, tx);
                }
            }
        }

        for asset in closed_assets {
            self.join_market_task(&asset).await;
        }
    }

    async fn send_cmd(&self, asset: String, cmd: MarketCommand) -> MarketCommandSendResult {
        if let Some(tx) = self.markets.get(&asset) {
            let tx = tx.clone();
            send_market_command(&asset, &tx, cmd, "Market").await
        } else {
            MarketCommandSendResult::Missing
        }
    }

    async fn handle_user_event_message(
        &mut self,
        msg: UserEventMessage,
        margin_book: &Arc<Mutex<MarginBook>>,
    ) {
        match msg {
            UserEventMessage::QuickNode(event) => {
                self.handle_quicknode_account_event(event, margin_book)
                    .await;
            }
            UserEventMessage::Sdk(msg) => self.handle_sdk_user_message(msg, margin_book).await,
        }
    }

    async fn handle_sdk_user_message(
        &mut self,
        msg: Message,
        margin_book: &Arc<Mutex<MarginBook>>,
    ) {
        if let Message::User(user_event) = msg {
            match user_event.data {
                UserData::Fills(fills_vec) => {
                    self.handle_user_fills(fills_vec).await;
                }
                UserData::Funding(funding_update) => {
                    self.handle_user_funding(funding_update.coin, funding_update.usdc)
                        .await;
                }
                _ => {}
            }
        } else if let Message::UserNonFundingLedgerUpdates(updates) = msg {
            self.handle_user_non_funding_ledger_updates(
                updates.data.non_funding_ledger_updates,
                margin_book,
            )
            .await;
        } else if let Message::NoData = msg {
            self.send_to_frontend(UpdateFrontend::Status(BackendStatus::Offline))
                .await;
        }
    }

    async fn handle_quicknode_account_event(
        &mut self,
        event: AccountEvent,
        margin_book: &Arc<Mutex<MarginBook>>,
    ) {
        match event {
            AccountEvent::Fill(fills) => {
                self.handle_user_fills(fills.into_iter().map(|event| event.fill).collect())
                    .await;
            }
            AccountEvent::Funding(fundings) => {
                for event in fundings {
                    self.handle_user_funding(event.funding.coin, event.funding.usdc)
                        .await;
                }
            }
            AccountEvent::NonFundingLedgerUpdates(updates) => {
                self.handle_user_non_funding_ledger_updates(
                    updates.into_iter().map(|event| event.update).collect(),
                    margin_book,
                )
                .await;
            }
            AccountEvent::Raw {
                stream_type,
                payload,
            } => {
                warn!("Unhandled QuickNode account event stream={stream_type:?} payload={payload}");
            }
            AccountEvent::Error(err) => {
                warn!("QuickNode account event stream error: {err}");
            }
            AccountEvent::NoData => {
                self.send_to_frontend(UpdateFrontend::Status(BackendStatus::Offline))
                    .await;
            }
        }
    }

    async fn handle_user_non_funding_ledger_updates(
        &self,
        updates: Vec<LedgerUpdateData>,
        margin_book: &Arc<Mutex<MarginBook>>,
    ) {
        if updates.iter().any(ledger_update_affects_margin) {
            self.sync_margin_book(margin_book).await;
        }
    }

    async fn sync_margin_book(&self, margin_book: &Arc<Mutex<MarginBook>>) {
        let result = MarginBook::sync_total_if_stale_shared(
            margin_book,
            Duration::from_secs(MARGIN_SYNC_MIN_INTERVAL_SECS),
        )
        .await;

        match result {
            Ok(total) => {
                self.send_to_frontend(UpdateFrontend::UpdateTotalMargin(total))
                    .await;
            }
            Err(e) => {
                warn!("Failed to fetch User Margin");
                self.send_to_frontend(UpdateFrontend::UserError(format!(
                    "Failed to fetch user margin: {e}"
                )))
                .await;
            }
        }
    }

    async fn handle_user_fills(&mut self, fills_vec: Vec<HLTradeInfo>) {
        let mut fills_map: FillsMap = HashMap::default();

        for trade in fills_vec.into_iter() {
            let coin = trade.coin.clone();
            let oid = trade.oid;
            fills_map
                .entry(coin)
                .or_default()
                .entry(oid)
                .or_default()
                .push(trade);
        }

        for (coin, map) in fills_map.into_iter() {
            for (_oid, fills) in map.into_iter() {
                match TradeFillInfo::try_from(fills) {
                    Ok(fill) => {
                        let cmd = MarketCommand::UserEvent(ExecEvent::Fill(fill));
                        tokio::task::yield_now().await;
                        self.send_cmd(coin.clone(), cmd).await;
                    }
                    Err(e) => {
                        warn!(
                            "Failed to aggregate TradeFillInfo for {} market: {}",
                            coin, e
                        );
                    }
                }
            }
        }
    }

    async fn handle_user_funding(&mut self, coin: String, usdc: String) {
        match parse_user_funding(&usdc) {
            Ok(fd) => {
                let cmd = MarketCommand::UserEvent(ExecEvent::Funding(fd));
                self.send_cmd(coin, cmd).await;
            }
            Err(err) => warn!("{err}"),
        }
    }

    async fn shutdown_runtime(
        &mut self,
        session: &Session,
        margin_book: &Arc<Mutex<MarginBook>>,
        cancel_token: &CancellationToken,
        margin_book_handle: &mut Option<JoinHandle<()>>,
        close_markets: bool,
    ) {
        if let Some(relay) = &self.user_event_relay {
            relay.unsubscribe(self.wallet.pubkey);
        }

        if close_markets {
            self.close_all().await;
        }
        self.join_all_market_tasks().await;
        self.drop_all_asset_feeds().await;

        {
            let mut guard = session.lock().await;
            guard.clear();
        }

        let mut book = margin_book.lock().await;
        book.reset();
        drop(book);

        let _ = info_shutdown_ws_timeout("bot", &mut self.info_client).await;
        cancel_token.cancel();

        if let Some(handle) = margin_book_handle.take() {
            let _ = handle.await;
        }
    }

    pub async fn start(
        mut self,
        ws_connections: WsConnections,
        pubkey: String,
        pool: PgPool,
        rhai_engine: Arc<Engine>,
        strategy_cache: StrategyCache,
    ) -> Result<(), Error> {
        use BotEvent::*;
        use MarketUpdate as M;
        use UpdateFrontend::*;

        self.ws_connections = Some(ws_connections.clone());
        self.pubkey = Some(pubkey.clone());
        self.pool = Some(pool);
        self.rhai_engine = Some(rhai_engine);
        self.strategy_cache = Some(strategy_cache);
        self.refresh_builder_approval_status(&pubkey).await;

        let Some(mut update_rv) = self.update_rv.take() else {
            return Err(Error::Custom("bot update receiver missing".to_string()));
        };
        let Some(mut price_router_rv) = self.price_router_rv.take() else {
            return Err(Error::Custom("bot price receiver missing".to_string()));
        };

        let session: Session = Arc::new(Mutex::new(HashMap::default()));

        let (margin_sync_reset_tx, mut margin_sync_reset_rx) = channel::<()>(1);
        let user = self.wallet.clone();
        let margin_book = MarginBook::new(user, Some(margin_sync_reset_tx));
        let margin_arc = Arc::new(Mutex::new(margin_book));
        let margin_user_edit = margin_arc.clone();
        let margin_market_edit = margin_arc.clone();
        let cancel_token = CancellationToken::new();

        // Margin sync fallback — sends SyncMargin 30s after the last successful margin sync.
        let margin_token = cancel_token.clone();
        let margin_tx = self._bot_tx.clone();
        let margin_ws = ws_connections.clone();
        let margin_pk = pubkey.clone();
        let margin_timer_book = Arc::clone(&margin_arc);
        let margin_fallback_delay = margin_sync_fallback_delay(&pubkey);
        let margin_book_handle = tokio::spawn(async move {
            loop {
                let timer = sleep(margin_fallback_delay);
                tokio::pin!(timer);

                tokio::select! {
                    _ = margin_token.cancelled() => { break; }
                    reset = margin_sync_reset_rx.recv() => {
                        if reset.is_none() {
                            break;
                        }
                        while margin_sync_reset_rx.try_recv().is_ok() {}
                    }
                    _ = &mut timer => {
                        let has_conn = {
                            let conns = margin_ws.read().await;
                            conns.get(&margin_pk).is_some_and(|v| !v.is_empty())
                        };
                        let has_margin_allocations = {
                            let book = margin_timer_book.lock().await;
                            !book.is_empty()
                        };
                        if has_conn && has_margin_allocations {
                            queue_bot_event(&margin_tx, BotEvent::SyncMargin, "SyncMargin").await;
                        }
                    }
                }
            }
        });
        let mut margin_book_handle = Some(margin_book_handle);

        // MarketUpdate relay task — broadcasts to user via WsConnections
        let session_upd = session.clone();
        let upd_ws = ws_connections.clone();
        let upd_pk = pubkey.clone();
        let upd_bot_tx = self._bot_tx.clone();
        let trade_persist_tx = self.pool.clone().map(|pool| {
            let (tx, rx) = channel::<TradePersistence>(TRADE_PERSIST_QUEUE_SIZE);
            spawn_trade_persistence_worker(pool, pubkey.clone(), rx);
            tx
        });

        tokio::spawn(async move {
            let mut trade_persist_queue_full = false;
            let mut trade_persist_dropped = 0_u64;
            while let Some(market_update) = update_rv.recv().await {
                match market_update {
                    M::InitMarket(info) => {
                        let state = MarketState::from(&info);
                        {
                            let mut guard = session_upd.lock().await;
                            guard.insert(info.asset.clone(), state);
                        }
                        broadcast_to_user(&upd_ws, &upd_pk, ConfirmMarket(info)).await;
                    }

                    M::MarginUpdate(asset_margin) => {
                        let (asset, margin) = asset_margin.clone();

                        let result = MarginBook::update_asset_shared(
                            &margin_market_edit,
                            asset_margin.clone(),
                        )
                        .await;

                        match result {
                            Ok(_) => {
                                {
                                    let mut guard = session_upd.lock().await;
                                    if let Some(s) = guard.get_mut(&asset) {
                                        s.margin = margin;
                                    }
                                }
                                broadcast_to_user(
                                    &upd_ws,
                                    &upd_pk,
                                    UpdateMarketMargin(asset_margin),
                                )
                                .await;
                            }
                            Err(e) => {
                                broadcast_to_user(&upd_ws, &upd_pk, UserError(e.to_string())).await;
                            }
                        }
                    }

                    M::MarketInfoUpdate((asset, edit)) => {
                        let mut trade_to_persist = None;
                        let edit = {
                            let mut guard = session_upd.lock().await;
                            if let Some(s) = guard.get_mut(&asset) {
                                match edit {
                                    crate::EditMarketInfo::Lev(lev) => {
                                        s.lev = lev;
                                        crate::EditMarketInfo::Lev(lev)
                                    }
                                    crate::EditMarketInfo::OpenPosition(pos) => {
                                        s.position = pos;
                                        crate::EditMarketInfo::OpenPosition(pos)
                                    }
                                    crate::EditMarketInfo::EngineState(view) => {
                                        s.engine_state = view;
                                        crate::EditMarketInfo::EngineState(view)
                                    }
                                    crate::EditMarketInfo::Paused(paused) => {
                                        s.is_paused = paused;
                                        crate::EditMarketInfo::Paused(paused)
                                    }
                                    crate::EditMarketInfo::Trade(mut trade) => {
                                        trade.strategy = Some(s.strategy_name.clone());
                                        s.pnl += trade.pnl;
                                        s.trades.push_back(trade.clone());
                                        trade_to_persist = Some((asset.clone(), trade.clone()));

                                        crate::EditMarketInfo::Trade(trade)
                                    }
                                }
                            } else {
                                edit
                            }
                        };
                        if let (Some(tx), Some((asset, trade))) =
                            (trade_persist_tx.as_ref(), trade_to_persist)
                        {
                            queue_trade_persistence(
                                tx,
                                TradePersistence { asset, trade },
                                &mut trade_persist_queue_full,
                                &mut trade_persist_dropped,
                            );
                        }
                        broadcast_to_user(&upd_ws, &upd_pk, MarketInfoEdit((asset, edit))).await;
                    }

                    M::RelayToFrontend(cmd) => {
                        broadcast_to_user(&upd_ws, &upd_pk, cmd).await;
                    }

                    M::AuthFailed(msg) => {
                        log::error!("[bot] auth failed: {msg} — notifying main loop");
                        queue_bot_event(&upd_bot_tx, BotEvent::AuthFailed(msg), "AuthFailed").await;
                    }

                    M::BuilderApprovalFailed(msg) => {
                        log::error!(
                            "[bot] builder fee approval failed: {msg} — notifying main loop"
                        );
                        queue_bot_event(
                            &upd_bot_tx,
                            BotEvent::BuilderApprovalFailed(msg),
                            "BuilderApprovalFailed",
                        )
                        .await;
                    }

                    M::FeedDied(asset) => {
                        log::warn!("[bot] price feed died for {asset} — removing market");
                        broadcast_to_user(
                            &upd_ws,
                            &upd_pk,
                            UserError(format!(
                                "Price feed lost for {}. Market removed — re-add when ready.",
                                asset
                            )),
                        )
                        .await;
                        queue_bot_event(&upd_bot_tx, BotEvent::RemoveMarket(asset), "RemoveMarket")
                            .await;
                    }
                }
            }
        });

        let (user_tx, mut user_rv) = channel::<UserEventMessage>(USER_EVENT_QUEUE_SIZE);
        let mut relay_user_events = false;

        if let Some(relay) = self.user_event_relay.clone() {
            match relay.subscribe(self.wallet.pubkey).await {
                Ok(mut quicknode_rx) => {
                    log::info!("Subscribed to shared QuickNode account event relay");
                    let user_tx = user_tx.clone();
                    let relay_token = cancel_token.clone();
                    tokio::spawn(async move {
                        let mut queue_full = false;
                        let mut dropped = 0_u64;
                        loop {
                            tokio::select! {
                                _ = relay_token.cancelled() => break,
                                event = quicknode_rx.recv() => {
                                    let Some(event) = event else {
                                        break;
                                    };

                                    match user_tx.try_send(UserEventMessage::QuickNode(event)) {
                                        Ok(()) => {
                                            if queue_full {
                                                log::info!(
                                                    "QuickNode account event queue recovered after dropping {dropped} events"
                                                );
                                                queue_full = false;
                                                dropped = 0;
                                            }
                                        }
                                        Err(TrySendError::Full(_)) => {
                                            metrics::inc_quicknode_account_queue_dropped();
                                            dropped = dropped.saturating_add(1);
                                            if !queue_full {
                                                warn!("QuickNode account event queue full; dropping events");
                                                queue_full = true;
                                            }
                                        }
                                        Err(TrySendError::Closed(_)) => break,
                                    }
                                }
                            }
                        }
                    });
                    relay_user_events = true;
                }
                Err(err) => {
                    warn!(
                        "Failed to subscribe to shared QuickNode account event relay, falling back to SDK websocket: {err}"
                    );
                }
            }
        }

        if !relay_user_events {
            let (sdk_tx, mut sdk_rx) = unbounded_channel();
            let sdk_token = cancel_token.clone();
            let _id = info_subscribe_timeout(
                "SDK user events",
                &mut self.info_client,
                Subscription::UserEvents {
                    user: self.wallet.pubkey,
                },
                sdk_tx.clone(),
            )
            .await?;
            let _ledger_id = info_subscribe_timeout(
                "SDK non-funding ledger updates",
                &mut self.info_client,
                Subscription::UserNonFundingLedgerUpdates {
                    user: self.wallet.pubkey,
                },
                sdk_tx.clone(),
            )
            .await?;

            tokio::spawn(async move {
                let mut queue_full = false;
                let mut dropped = 0_u64;
                loop {
                    tokio::select! {
                        _ = sdk_token.cancelled() => break,
                        msg = sdk_rx.recv() => {
                            let Some(msg) = msg else {
                                break;
                            };

                            match user_tx.try_send(UserEventMessage::Sdk(msg)) {
                                Ok(()) => {
                                    if queue_full {
                                        log::info!(
                                            "SDK user-event queue recovered after dropping {dropped} events"
                                        );
                                        queue_full = false;
                                        dropped = 0;
                                    }
                                }
                                Err(TrySendError::Full(_)) => {
                                    metrics::inc_sdk_account_queue_dropped();
                                    dropped = dropped.saturating_add(1);
                                    if !queue_full {
                                        warn!(
                                            "SDK user-event queue full; dropping fallback account events"
                                        );
                                        queue_full = true;
                                    }
                                }
                                Err(TrySendError::Closed(_)) => break,
                            }
                        }
                    }
                }
            });
        }

        let mut empty_market_idle_since = self.markets.is_empty().then(Instant::now);
        let mut empty_market_idle_check =
            tokio::time::interval(Duration::from_secs(EMPTY_MARKET_IDLE_CHECK_SECS));
        empty_market_idle_check.tick().await;

        loop {
            tokio::select!(
                biased;

                _ = empty_market_idle_check.tick() => {
                    if empty_market_idle_expired(
                        self.markets.is_empty(),
                        &mut empty_market_idle_since,
                        Instant::now(),
                        Duration::from_secs(EMPTY_MARKET_IDLE_TIMEOUT_SECS),
                    ) {
                        log::info!(
                            "Bot for user {pubkey} has tracked no markets for {} seconds; shutting down",
                            EMPTY_MARKET_IDLE_TIMEOUT_SECS
                        );
                        self.shutdown_runtime(
                            &session,
                            &margin_user_edit,
                            &cancel_token,
                            &mut margin_book_handle,
                            false,
                        )
                        .await;
                        return Ok(());
                    }
                },

                Some(msg) = user_rv.recv() => {
                    self.handle_user_event_message(msg, &margin_user_edit).await;
                },

                Some((asset, data)) = price_router_rv.recv() => {
                    self.route_price(asset, data);
                },

                Some(event) = self.bot_rv.recv() => {
                    // Block trading commands when key is invalid
                    if !self.key_valid {
                        match &event {
                            ResumeMarket(_) | ResumeAll => {
                                self.send_to_frontend(UserError(
                                    "API key expired or revoked. Please re-authorize in Settings.".to_string(),
                                )).await;
                                self.send_to_frontend(NeedsApiKey(true)).await;
                                continue;
                            }
                            // Allow ReloadWallet, SyncMargin, PauseAll, PauseMarket, RemoveMarket, GetSession, Kill, etc.
                            _ => {}
                        }
                    }
                    if !self.builder_approved {
                        match &event {
                            ResumeMarket(_) | ResumeAll => {
                                self.send_to_frontend(UserError(
                                    "Builder fee has not been approved. Please approve builder fees in Settings.".to_string(),
                                )).await;
                                self.send_to_frontend(NeedsBuilderApproval(true)).await;
                                continue;
                            }
                            _ => {}
                        }
                    }
                    match event {
                        AddMarket(add_market_info) => {
                            let asset = add_market_info.asset.clone();
                            match self.add_market(add_market_info, &margin_user_edit).await {
                                Ok(()) => {
                                    refresh_empty_market_idle(
                                        self.markets.is_empty(),
                                        &mut empty_market_idle_since,
                                        Instant::now(),
                                    );
                                }
                                Err(e) => {
                                    self.send_to_frontend(UserError(format!("FAILED TO ADD MARKET: {}", e))).await;
                                    self.send_to_frontend(CancelMarket(asset)).await;
                                }
                            }
                        }

                        ResumeMarket(asset) => {
                            match MarginBook::sync_shared(&margin_user_edit).await {
                                Ok(positions) => {
                                    if positions.iter().any(|p| p.position.coin == asset) {
                                        self.send_to_frontend(UserError(format!(
                                            "Cannot resume {}: close the on-chain position first",
                                            &asset
                                        ))).await;
                                        continue;
                                    }
                                }
                                Err(e) => {
                                    self.send_to_frontend(UserError(format!(
                                        "Failed to check on-chain positions: {}", e
                                    ))).await;
                                    continue;
                                }
                            }
                            match self.send_cmd(asset.clone(), MarketCommand::Resume).await {
                                MarketCommandSendResult::Sent => {}
                                MarketCommandSendResult::TimedOut => {
                                    self.send_to_frontend(UserError(format!(
                                        "Resume failed: {} market command queue is full",
                                        &asset
                                    ))).await;
                                    continue;
                                }
                                MarketCommandSendResult::Closed | MarketCommandSendResult::Missing => {
                                    self.send_to_frontend(UserError(format!(
                                        "Resume failed: {} market is not available",
                                        &asset
                                    ))).await;
                                    continue;
                                }
                            }
                            let mut guard = session.lock().await;
                            if let Some(s) = guard.get_mut(&asset) {
                                s.is_paused = false;
                            }
                            drop(guard);
                            broadcast_to_user(&ws_connections, &pubkey, MarketInfoEdit((
                                asset, crate::EditMarketInfo::Paused(false),
                            ))).await;
                        }

                        PauseMarket(asset) => {
                            match self.send_cmd(asset.clone(), MarketCommand::Pause).await {
                                MarketCommandSendResult::Sent => {}
                                MarketCommandSendResult::TimedOut => {
                                    self.send_to_frontend(UserError(format!(
                                        "Pause failed: {} market command queue is full",
                                        &asset
                                    ))).await;
                                    continue;
                                }
                                MarketCommandSendResult::Closed | MarketCommandSendResult::Missing => {
                                    self.send_to_frontend(UserError(format!(
                                        "Pause failed: {} market is not available",
                                        &asset
                                    ))).await;
                                    continue;
                                }
                            }
                            let mut guard = session.lock().await;
                            if let Some(s) = guard.get_mut(&asset) {
                                s.is_paused = true;
                            }
                            drop(guard);
                            broadcast_to_user(&ws_connections, &pubkey, MarketInfoEdit((
                                asset, crate::EditMarketInfo::Paused(true),
                            ))).await;
                        }

                        RemoveMarket(asset) => {
                            match self.remove_market(asset.as_str(), &margin_user_edit).await {
                                Ok(()) => {
                                    let mut guard = session.lock().await;
                                    let _ = guard.remove(&asset);
                                    refresh_empty_market_idle(
                                        self.markets.is_empty(),
                                        &mut empty_market_idle_since,
                                        Instant::now(),
                                    );
                                }
                                Err(e) => {
                                    self.send_to_frontend(UserError(format!(
                                        "Failed to remove {} market: {}",
                                        asset, e
                                    ))).await;
                                }
                            }
                        }

                        SyncMarketFeeds(payload) => {
                            let result = self
                                .sync_market_feeds(
                                    payload.market.as_str(),
                                    payload.required_assets.into_iter().collect(),
                                )
                                .await;
                            let _ = payload.reply.send(result);
                        }

                        AssetFeedDied(asset) => {
                            let asset_key = Arc::<str>::from(asset.as_str());
                            let affected_markets: Vec<_> = self
                                .asset_consumers
                                .get(&asset_key)
                                .map(|markets| markets.iter().cloned().collect())
                                .unwrap_or_default();
                            let _ = self.drop_asset_feed(&asset_key).await;
                            self.asset_consumers.remove(&asset_key);

                            if !affected_markets.is_empty() {
                                self.send_to_frontend(UserError(format!(
                                    "Price feed lost for {}. Removing affected markets.",
                                    asset
                                )))
                                .await;
                            }

                            for market in affected_markets {
                                match self.remove_market(market.as_str(), &margin_user_edit).await {
                                    Ok(()) => {
                                        let mut guard = session.lock().await;
                                        let _ = guard.remove(&market);
                                        refresh_empty_market_idle(
                                            self.markets.is_empty(),
                                            &mut empty_market_idle_since,
                                            Instant::now(),
                                        );
                                    }
                                    Err(e) => {
                                        self.send_to_frontend(UserError(format!(
                                            "Failed to remove {} after feed loss: {}",
                                            market, e
                                        ))).await;
                                    }
                                }
                            }
                        }

                        MarketComm(command) => {
                            if !self.key_valid && matches!(command.cmd, MarketCommand::Resume) {
                                self.send_to_frontend(UserError(
                                    "API key expired or revoked. Please re-authorize in Settings."
                                        .to_string(),
                                ))
                                .await;
                                self.send_to_frontend(NeedsApiKey(true)).await;
                                continue;
                            }
                            if !self.builder_approved
                                && matches!(command.cmd, MarketCommand::Resume)
                            {
                                self.send_to_frontend(UserError(
                                    "Builder fee has not been approved. Please approve builder fees in Settings.".to_string(),
                                ))
                                .await;
                                self.send_to_frontend(NeedsBuilderApproval(true)).await;
                                continue;
                            }
                            if let MarketCommand::UpdateStrategy(_, _, ref name) = command.cmd {
                                let mut guard = session.lock().await;
                                if let Some(s) = guard.get_mut(&command.asset) {
                                    s.strategy_name = name.clone();
                                }
                            } else if let MarketCommand::UpdateLeverage(_lev) = command.cmd {
                                let has_position = {
                                    let guard = session.lock().await;
                                    guard
                                        .get(&command.asset)
                                        .is_some_and(|s| s.position.is_some())
                                };
                                if has_position {
                                    self.send_to_frontend(
                                        UserError(format!(
                                            "Leverage update failed: {} market has open order(s)", &command.asset)
                                            )).await;
                                    continue;
                                }
                            }

                            self.send_cmd(command.asset, command.cmd).await;
                        }

                        UpdateMarketStrategy(payload) => {
                            let rhai_engine = match self.rhai_engine.as_ref() {
                                Some(e) => e.clone(),
                                None => {
                                    self.send_to_frontend(UserError("Engine not initialized".into())).await;
                                    continue;
                                }
                            };

                            let (compiled, indicators, name) = if let Some(sid) = payload.strategy_id {
                                let cache = match self.strategy_cache.as_ref() {
                                    Some(c) => c.clone(),
                                    None => {
                                        self.send_to_frontend(UserError("Strategy cache not initialized".into())).await;
                                        continue;
                                    }
                                };

                                let cached = { cache.read().await.get(&sid).cloned() };
                                if let Some(entry) = cached {
                                    (entry.compiled, entry.indicators, entry.name)
                                } else {
                                    let pool = match self.pool.as_ref() {
                                        Some(p) => p,
                                        None => {
                                            self.send_to_frontend(UserError("DB not initialized".into())).await;
                                            continue;
                                        }
                                    };
                                    let row = match timeout(
                                        Duration::from_secs(BOT_DB_QUERY_TIMEOUT_SECS),
                                        sqlx::query_as::<_, crate::backend::db::StrategyRow>(
                                            "SELECT * FROM strategies WHERE id = $1",
                                        )
                                        .bind(sid)
                                        .fetch_optional(pool),
                                    )
                                    .await
                                    {
                                        Ok(Ok(Some(r))) => r,
                                        Ok(Ok(None)) => {
                                            self.send_to_frontend(UserError(format!(
                                                "Strategy {} not found",
                                                sid
                                            )))
                                            .await;
                                            continue;
                                        }
                                        Ok(Err(e)) => {
                                            self.send_to_frontend(UserError(format!("DB error: {e}")))
                                                .await;
                                            continue;
                                        }
                                        Err(_) => {
                                            self.send_to_frontend(UserError(format!(
                                                "Timed out fetching strategy {}",
                                                sid
                                            )))
                                            .await;
                                            continue;
                                        }
                                    };
                                    let state_decls: Option<crate::backend::scripting::StateDeclarations> =
                                        match row
                                            .state_declarations
                                            .as_ref()
                                            .map(|v| serde_json::from_value(v.clone()))
                                        {
                                            Some(Ok(decls)) => Some(decls),
                                            Some(Err(e)) => {
                                                self.send_to_frontend(UserError(format!(
                                                    "Strategy has invalid state declarations: {e}"
                                                ))).await;
                                                continue;
                                            }
                                            None => None,
                                        };
                                    let compiled = match crate::backend::scripting::compile_strategy(
                                        &rhai_engine, &row.on_idle, &row.on_open, &row.on_busy, state_decls.as_ref(),
                                    ) {
                                        Ok(c) => c,
                                        Err(e) => {
                                            self.send_to_frontend(UserError(format!("Strategy failed to compile: {e}"))).await;
                                            continue;
                                        }
                                    };
                                    let indicators: Vec<crate::IndexId> =
                                        match serde_json::from_value(row.indicators) {
                                            Ok(indicators) => indicators,
                                            Err(e) => {
                                                self.send_to_frontend(UserError(format!(
                                                    "Strategy has invalid indicators: {e}"
                                                ))).await;
                                                continue;
                                            }
                                        };
                                    {
                                        let mut guard = cache.write().await;
                                        guard.insert(sid, crate::backend::app_state::CachedStrategy {
                                            compiled: compiled.clone(),
                                            indicators: indicators.clone(),
                                            state_declarations: state_decls.clone(),
                                            name: row.name.clone(),
                                        });
                                    }
                                    (compiled, indicators, row.name)
                                }
                            } else {
                                let noop = crate::backend::scripting::CompiledStrategy::noop(&rhai_engine);
                                (noop, vec![], "View Only".to_string())
                            };

                            {
                                let mut guard = session.lock().await;
                                if let Some(s) = guard.get_mut(&payload.asset) {
                                    s.strategy_name = name.clone();
                                }
                            }

                            self.send_cmd(
                                payload.asset,
                                MarketCommand::UpdateStrategy(compiled, indicators, name),
                            ).await;
                        }

                        ManualUpdateMargin(asset_margin) => {
                            let asset = asset_margin.0.clone();

                            let margin_update_blocked = {
                                let guard = session.lock().await;
                                guard.get(&asset).map(|s| {
                                    matches!(
                                        s.engine_state,
                                        EngineView::Open
                                            | EngineView::Opening
                                            | EngineView::Closing
                                    )
                                })
                            };

                            match margin_update_blocked {
                                Some(true) => {
                                    self.send_to_frontend(
                                        UserError(format!(
                                            "Margin update failed: {} market has open order(s)", &asset)
                                            )).await;
                                    continue;
                                }
                                Some(false) => {}
                                None => continue,
                            }

                            let result =
                                MarginBook::update_asset_shared(&margin_user_edit, asset_margin.clone())
                                    .await;

                            match result {
                                Ok(new_margin) => {
                                    {
                                        let mut guard = session.lock().await;
                                        if let Some(s) = guard.get_mut(&asset) {
                                            s.margin = new_margin;
                                        }
                                    }
                                    self.send_to_frontend(UpdateMarketMargin((asset.clone(), new_margin))).await;
                                    let cmd = MarketCommand::UpdateMargin(new_margin);
                                    self.send_cmd(asset, cmd).await;
                                }

                                Err(e) => {
                                    self.send_to_frontend(UserError(e.to_string())).await;
                                }
                            }
                        }

                        SyncMargin => {
                            self.sync_margin_book(&margin_user_edit).await;
                        }

                        ReloadWallet(new_signer) => {
                            log::info!("[bot] reloading wallet for all {} markets", self.markets.len());
                            let wallet = match Wallet::new(BaseUrl::Mainnet, self.wallet.pubkey, new_signer.clone()).await {
                                Ok(wallet) => wallet,
                                Err(e) => {
                                    log::error!("[bot] failed to reload wallet: {e}");
                                    self.key_valid = false;
                                    self.send_to_frontend(NeedsApiKey(true)).await;
                                    self.send_to_frontend(UserError(format!(
                                        "API key reload failed: {e}. Please re-authorize in Settings.",
                                    ))).await;
                                    continue;
                                }
                            };
                            self.wallet = Arc::new(wallet);
                            for (asset, tx) in &self.markets {
                                if send_market_command(
                                    asset,
                                    tx,
                                    MarketCommand::ReloadWallet(new_signer.clone()),
                                    "ReloadWallet",
                                )
                                .await
                                    == MarketCommandSendResult::TimedOut
                                {
                                    log::error!(
                                        "[bot] timed out reloading wallet for market {asset}"
                                    );
                                }
                            }
                            self.key_valid = true;
                            self.send_to_frontend(NeedsApiKey(false)).await;
                            log::info!("[bot] wallet reloaded, key_valid restored");
                        }

                        AuthFailed(msg) => {
                            log::error!("[bot] auth failed: {msg} — pausing all markets");
                            self.key_valid = false;
                            self.send_to_frontend(NeedsApiKey(true)).await;
                            self.send_to_frontend(UserError(format!(
                                "API key rejected: {msg}. Please re-authorize in Settings.",
                            ))).await;
                            // Pause all markets
                            let paused = self.pause_all().await;
                            let paused_set: HashSet<_> = paused.iter().cloned().collect();
                            let mut guard = session.lock().await;
                            for (asset, s) in guard.iter_mut() {
                                if paused_set.contains(asset) {
                                    s.is_paused = true;
                                }
                            }
                            drop(guard);
                            for asset in &paused {
                                broadcast_to_user(&ws_connections, &pubkey, MarketInfoEdit((
                                    asset.clone(), crate::EditMarketInfo::Paused(true),
                                ))).await;
                            }

                        }

                        ResumeAll => {
                            let blocked: Vec<String> = match MarginBook::sync_shared(&margin_user_edit).await {
                                Ok(positions) => positions
                                    .iter()
                                    .filter(|p| self.markets.contains_key(&p.position.coin))
                                    .map(|p| p.position.coin.clone())
                                    .collect(),
                                Err(e) => {
                                    self.send_to_frontend(UserError(format!(
                                        "Failed to check on-chain positions: {}", e
                                    ))).await;
                                    continue;
                                }
                            };

                            let blocked_set: HashSet<_> = blocked.iter().cloned().collect();
                            let mut resumed: Vec<String> = Vec::new();
                            let mut timed_out: Vec<String> = Vec::new();
                            for (asset, tx) in self.markets.iter() {
                                if !blocked_set.contains(asset) {
                                    match send_market_command(
                                        asset,
                                        tx,
                                        MarketCommand::Resume,
                                        "Resume",
                                    )
                                    .await
                                    {
                                        MarketCommandSendResult::Sent => {
                                            resumed.push(asset.clone());
                                        }
                                        MarketCommandSendResult::TimedOut => {
                                            timed_out.push(asset.clone());
                                        }
                                        MarketCommandSendResult::Closed
                                        | MarketCommandSendResult::Missing => {}
                                    }
                                }
                            }

                            let resumed_set: HashSet<_> = resumed.iter().cloned().collect();
                            let mut guard = session.lock().await;
                            for (asset, s) in guard.iter_mut() {
                                if blocked_set.contains(asset) {
                                    continue;
                                }
                                if resumed_set.contains(asset) {
                                    s.is_paused = false;
                                }
                            }
                            drop(guard);

                            for asset in resumed {
                                broadcast_to_user(&ws_connections, &pubkey, MarketInfoEdit((
                                    asset, crate::EditMarketInfo::Paused(false),
                                ))).await;
                            }

                            if !blocked.is_empty() {
                                self.send_to_frontend(UserError(format!(
                                    "Cannot resume {}: close on-chain positions first",
                                    blocked.join(", ")
                                ))).await;
                            }

                            if !timed_out.is_empty() {
                                self.send_to_frontend(UserError(format!(
                                    "Resume timed out for {}: market command queue is full",
                                    timed_out.join(", ")
                                ))).await;
                            }
                        }

                        BuilderApprovalFailed(msg) => {
                            log::error!(
                                "[bot] builder fee approval failed: {msg} — pausing all markets"
                            );
                            self.builder_approved = false;
                            self.send_to_frontend(NeedsBuilderApproval(true)).await;
                            self.send_to_frontend(UserError(
                                "Builder fee has not been approved. Please approve builder fees in Settings.".to_string(),
                            ))
                            .await;
                            let paused = self.pause_all().await;
                            let paused_set: HashSet<_> = paused.iter().cloned().collect();
                            let mut guard = session.lock().await;
                            for (asset, s) in guard.iter_mut() {
                                if paused_set.contains(asset) {
                                    s.is_paused = true;
                                }
                            }
                            drop(guard);
                            for asset in &paused {
                                broadcast_to_user(
                                    &ws_connections,
                                    &pubkey,
                                    MarketInfoEdit((
                                        asset.clone(),
                                        crate::EditMarketInfo::Paused(true),
                                    )),
                                )
                                .await;
                            }
                        }

                        BuilderApproved => {
                            self.builder_approved = true;
                            self.send_to_frontend(NeedsBuilderApproval(false)).await;
                            log::info!("[bot] builder fee approval restored");
                        }

                        PauseAll => {
                            let paused = self.pause_all().await;
                            let paused_set: HashSet<_> = paused.iter().cloned().collect();
                            let mut guard = session.lock().await;
                            for (asset, s) in guard.iter_mut() {
                                if paused_set.contains(asset) {
                                    s.is_paused = true;
                                }
                            }
                            drop(guard);
                            for asset in &paused {
                                broadcast_to_user(&ws_connections, &pubkey, MarketInfoEdit((
                                    asset.clone(), crate::EditMarketInfo::Paused(true),
                                ))).await;
                            }
                        }

                        CloseAll => {
                            self.close_all().await;
                            {
                                let mut guard = session.lock().await;
                                guard.clear();
                            }
                            let mut book = margin_user_edit.lock().await;
                            book.reset();
                            refresh_empty_market_idle(
                                self.markets.is_empty(),
                                &mut empty_market_idle_since,
                                Instant::now(),
                            );
                        }

                        GetSession => {
                            self.refresh_builder_approval_status(&pubkey).await;
                            let sess: Vec<MarketInfo> = {
                                let guard = session.lock().await;
                                guard.values().map(MarketInfo::from).collect()
                            };

                            let universe: Vec<AssetMeta> = match get_all_assets(&self.info_client).await {
                                Ok(u) => u,
                                Err(e) => {
                                    self.send_to_frontend(UserError(format!("Failed to fetch asset universe: {}", e))).await;
                                    Vec::new()
                                },
                            };

                            self.send_to_frontend(LoadSession(UserSession {
                                markets: sess,
                                universe,
                                agent_approved: self.key_valid,
                                builder_approved: self.builder_approved,
                            })).await;
                        }

                        Kill => {
                            self.shutdown_runtime(
                                &session,
                                &margin_user_edit,
                                &cancel_token,
                                &mut margin_book_handle,
                                true,
                            )
                            .await;
                            return Ok(());
                        }
                    }
                }
            )
        }
    }
}

type FillsMap = HashMap<
    String,
    HashMap<u64, Vec<HLTradeInfo>, BuildHasherDefault<FxHasher>>,
    BuildHasherDefault<FxHasher>,
>;

fn ledger_update_affects_margin(update: &LedgerUpdateData) -> bool {
    matches!(
        &update.delta,
        LedgerUpdate::Deposit(_)
            | LedgerUpdate::Withdraw(_)
            | LedgerUpdate::InternalTransfer(_)
            | LedgerUpdate::SubAccountTransfer(_)
            | LedgerUpdate::LedgerLiquidation(_)
            | LedgerUpdate::VaultDeposit(_)
            | LedgerUpdate::VaultCreate(_)
            | LedgerUpdate::VaultDistribution(_)
            | LedgerUpdate::VaultWithdraw(_)
            | LedgerUpdate::VaultLeaderCommission(_)
            | LedgerUpdate::AccountClassTransfer(_)
    )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BotEvent {
    AddMarket(AddMarketInfo),
    ResumeMarket(String),
    PauseMarket(String),
    RemoveMarket(String),
    MarketComm(BotToMarket),
    UpdateMarketStrategy(UpdateStrategyPayload),
    ManualUpdateMargin(AssetMargin),
    SyncMargin,
    #[serde(skip)]
    ReloadWallet(alloy::signers::local::PrivateKeySigner),
    #[serde(skip)]
    AuthFailed(String),
    #[serde(skip)]
    BuilderApprovalFailed(String),
    #[serde(skip)]
    BuilderApproved,
    #[serde(skip)]
    SyncMarketFeeds(SyncMarketFeeds),
    #[serde(skip)]
    AssetFeedDied(String),
    ResumeAll,
    PauseAll,
    CloseAll,
    GetSession,
    Kill,
}

#[derive(Debug)]
pub struct SyncMarketFeeds {
    pub market: String,
    pub required_assets: Vec<Arc<str>>,
    pub reply: oneshot::Sender<Result<(), Error>>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStrategyPayload {
    pub asset: String,
    pub strategy_id: Option<uuid::Uuid>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BotToMarket {
    pub asset: String,
    pub cmd: MarketCommand,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn market_command_send_queues_when_capacity_available() {
        let (tx, mut rx) = channel(1);

        let result = send_market_command_with_timeout(
            "BTC",
            &tx,
            MarketCommand::Close,
            "Close",
            Duration::from_millis(1),
        )
        .await;

        assert_eq!(result, MarketCommandSendResult::Sent);
        assert!(matches!(rx.recv().await, Some(MarketCommand::Close)));
    }

    #[tokio::test]
    async fn market_command_send_times_out_when_queue_stays_full() {
        let (tx, mut rx) = channel(1);
        tx.try_send(MarketCommand::Pause)
            .expect("first command should fit");

        let result = send_market_command_with_timeout(
            "BTC",
            &tx,
            MarketCommand::Resume,
            "Resume",
            Duration::from_millis(1),
        )
        .await;

        assert_eq!(result, MarketCommandSendResult::TimedOut);
        assert!(matches!(rx.recv().await, Some(MarketCommand::Pause)));
    }

    #[tokio::test]
    async fn market_command_send_reports_closed_channel() {
        let (tx, rx) = channel(1);
        drop(rx);

        let result = send_market_command_with_timeout(
            "BTC",
            &tx,
            MarketCommand::Close,
            "Close",
            Duration::from_millis(1),
        )
        .await;

        assert_eq!(result, MarketCommandSendResult::Closed);
    }

    #[test]
    fn margin_sync_fallback_delay_is_bounded_and_stable() {
        let first = margin_sync_fallback_delay("user-a");
        let second = margin_sync_fallback_delay("user-a");
        let other = margin_sync_fallback_delay("user-b");

        assert_eq!(first, second);
        assert!(first >= Duration::from_secs(MARGIN_SYNC_FALLBACK_SECS));
        assert!(
            first < Duration::from_secs(MARGIN_SYNC_FALLBACK_SECS + MARGIN_SYNC_STAGGER_MAX_SECS)
        );
        assert_ne!(first, other);
    }

    #[test]
    fn empty_market_idle_tracks_zero_market_duration() {
        let start = Instant::now();
        let timeout = Duration::from_secs(60);
        let mut idle_since = None;

        assert!(!empty_market_idle_expired(
            true,
            &mut idle_since,
            start,
            timeout
        ));
        assert_eq!(idle_since, Some(start));

        assert!(!empty_market_idle_expired(
            false,
            &mut idle_since,
            start + Duration::from_secs(30),
            timeout
        ));
        assert_eq!(idle_since, None);

        let empty_again = start + Duration::from_secs(40);
        assert!(!empty_market_idle_expired(
            true,
            &mut idle_since,
            empty_again,
            timeout
        ));
        assert_eq!(idle_since, Some(empty_again));

        assert!(empty_market_idle_expired(
            true,
            &mut idle_since,
            empty_again + timeout,
            timeout
        ));
    }

    #[test]
    fn parse_user_funding_rejects_non_finite_values() {
        assert_eq!(parse_user_funding("1.25").unwrap(), 1.25);
        assert!(parse_user_funding("NaN").is_err());
        assert!(parse_user_funding("inf").is_err());
        assert!(parse_user_funding("-inf").is_err());
    }
}
