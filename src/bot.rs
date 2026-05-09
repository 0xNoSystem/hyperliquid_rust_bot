use crate::{
    AddMarketInfo, BackendStatus, EngineView, ExecEvent, HLTradeInfo, Market, MarketCommand,
    MarketInfo, MarketState, MarketUpdate, TradeFillInfo, UpdateFrontend, Wallet,
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
use tokio::sync::mpsc::{
    Receiver, Sender, UnboundedReceiver, UnboundedSender, channel, unbounded_channel,
};
use tokio::sync::{Mutex, oneshot};
use tokio::task::JoinHandle;
use tokio::time::{Duration, sleep};
use tokio_util::sync::CancellationToken;

use crate::helper::*;
use crate::margin::{AssetMargin, MarginBook};

pub type Session = Arc<Mutex<HashMap<String, MarketState, BuildHasherDefault<FxHasher>>>>;

pub struct Bot {
    info_client: InfoClient,
    wallet: Arc<Wallet>,
    markets: HashMap<String, Sender<MarketCommand>, BuildHasherDefault<FxHasher>>,
    market_price_routes: HashMap<String, UnboundedSender<PriceAsset>, BuildHasherDefault<FxHasher>>,
    market_required_assets: HashMap<String, HashSet<Arc<str>>, BuildHasherDefault<FxHasher>>,
    asset_consumers: HashMap<Arc<str>, HashSet<String>, BuildHasherDefault<FxHasher>>,
    asset_feeds: HashMap<Arc<str>, BotAssetFeed, BuildHasherDefault<FxHasher>>,
    broadcast_tx: UnboundedSender<BroadcastCmd>,
    candle_rx: Sender<CacheCmdIn>,
    user_event_relay: Option<UserEventRelayHandle>,
    #[allow(unused)]
    fees: (f64, f64),
    _bot_tx: Sender<BotEvent>,
    bot_rv: Receiver<BotEvent>,
    price_router_rv: Option<UnboundedReceiver<PriceAsset>>,
    price_router_tx: UnboundedSender<PriceAsset>,
    update_rv: Option<UnboundedReceiver<MarketUpdate>>,
    update_tx: UnboundedSender<MarketUpdate>,
    ws_connections: Option<WsConnections>,
    pubkey: Option<String>,
    pool: Option<PgPool>,
    rhai_engine: Option<Arc<Engine>>,
    strategy_cache: Option<StrategyCache>,
    chain_open_positions: Vec<AssetPosition>,
    key_valid: bool,
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
        broadcast_tx: UnboundedSender<BroadcastCmd>,
        candle_rx: Sender<CacheCmdIn>,
        user_event_relay: Option<UserEventRelayHandle>,
    ) -> Result<(Self, Sender<BotEvent>), Error> {
        let info_client = InfoClient::with_reconnect(None, Some(wallet.url)).await?;
        let fees = wallet.get_user_fees().await?;

        let (bot_tx, bot_rv) = channel::<BotEvent>(64);
        let (price_router_tx, price_router_rv) = unbounded_channel::<PriceAsset>();
        let (update_tx, update_rv) = unbounded_channel::<MarketUpdate>();

        Ok((
            Self {
                info_client,
                wallet: wallet.into(),
                markets: HashMap::default(),
                market_price_routes: HashMap::default(),
                market_required_assets: HashMap::default(),
                asset_consumers: HashMap::default(),
                asset_feeds: HashMap::default(),
                broadcast_tx,
                candle_rx,
                user_event_relay,
                fees,
                _bot_tx: bot_tx.clone(),
                bot_rv,
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

    async fn ensure_asset_feed(&mut self, asset: Arc<str>) -> Result<AssetMeta, Error> {
        if let Some(feed) = self.asset_feeds.get(&asset) {
            return Ok(feed.meta.clone());
        }

        let (one_tx, one_rx) = oneshot::channel::<SubReply>();
        let sub_request = SubscribePayload {
            asset: Arc::clone(&asset),
            reply: one_tx,
        };

        self.broadcast_tx
            .send(BroadcastCmd::Subscribe(sub_request))
            .map_err(|e| Error::Custom(format!("broadcast channel closed: {}", e)))?;

        let sub_info = one_rx
            .await
            .map_err(|_| Error::Custom("subscription reply dropped".to_string()))??;

        let meta = sub_info.meta.clone();
        let mut px_receiver = sub_info.px_receiver;
        let price_router_tx = self.price_router_tx.clone();
        let bot_tx = self._bot_tx.clone();
        let asset_key = Arc::clone(&asset);

        let handle = tokio::spawn(async move {
            loop {
                match px_receiver.recv().await {
                    Ok(data) => {
                        if price_router_tx
                            .send((Arc::clone(&asset_key), data))
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        log::warn!("{} bot feed lagged by {} messages", &asset_key, n);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        let _ = bot_tx
                            .send(BotEvent::AssetFeedDied((*asset_key).to_string()))
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

        if self.drop_asset_feed(&asset).await
            && self
                .broadcast_tx
                .send(BroadcastCmd::Unsubscribe(Arc::clone(&asset)))
                .is_err()
        {
            warn!("failed to unsubscribe {} from broadcaster", &asset);
        }
    }

    fn route_price(&self, asset: Arc<str>, data: PriceData) {
        let Some(markets) = self.asset_consumers.get(&asset) else {
            return;
        };

        for market in markets {
            if let Some(tx) = self.market_price_routes.get(market) {
                let _ = tx.send((Arc::clone(&asset), data.clone()));
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

                let row = sqlx::query_as::<_, crate::backend::db::StrategyRow>(
                    "SELECT * FROM strategies WHERE id = $1",
                )
                .bind(sid)
                .fetch_optional(pool)
                .await
                .map_err(|e| Error::Custom(format!("DB error fetching strategy: {e}")))?
                .ok_or_else(|| Error::Custom(format!("strategy {sid} not found")))?;

                let state_decls: Option<crate::backend::scripting::StateDeclarations> = row
                    .state_declarations
                    .as_ref()
                    .and_then(|v| serde_json::from_value(v.clone()).ok());

                let compiled = crate::backend::scripting::compile_strategy(
                    &rhai_engine,
                    &row.on_idle,
                    &row.on_open,
                    &row.on_busy,
                    state_decls.as_ref(),
                )
                .map_err(|e| Error::Custom(format!("strategy {sid} failed to compile: {e}")))?;

                let indicators: Vec<crate::IndexId> =
                    serde_json::from_value(row.indicators).unwrap_or_default();

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

        let mut book = margin_book.lock().await;
        self.chain_open_positions = book.sync().await?;
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

        let margin = book.allocate(asset.clone(), margin_alloc).await?;
        drop(book);

        self.send_to_frontend(UpdateFrontend::PreconfirmMarket(asset.clone()))
            .await;

        let market_asset = Arc::<str>::from(asset.as_str());
        let had_feed = self.asset_feeds.contains_key(&market_asset);
        let meta = self.ensure_asset_feed(Arc::clone(&market_asset)).await?;
        let (price_tx, price_rx) = unbounded_channel::<PriceAsset>();

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

        let ws_conns = self.ws_connections.clone();
        let bot_pubkey = self.pubkey.clone();
        let remove_market_tx = self._bot_tx.clone();

        tokio::spawn(async move {
            if let Err(e) = market.start().await {
                if let (Some(conns), Some(pk)) = (&ws_conns, &bot_pubkey) {
                    broadcast_to_user(conns, pk, UpdateFrontend::CancelMarket(asset.clone())).await;
                    broadcast_to_user(
                        conns,
                        pk,
                        UpdateFrontend::UserError(format!(
                            "Market {} exited with error:\n {:?}",
                            &asset, e
                        )),
                    )
                    .await;
                }
                let _ = remove_market_tx
                    .send(BotEvent::RemoveMarket(asset.clone()))
                    .await;
            }
        });

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

        if let Some(tx) = self.markets.remove(&asset) {
            let tx = tx.clone();
            let cmd = MarketCommand::Close;
            let close = match tx.send(cmd).await {
                Ok(()) => true,
                Err(e) => {
                    log::warn!("Failed to send Close command: {:?}", e);
                    false
                }
            };

            if close {
                let mut book = margin_book.lock().await;
                book.remove(&asset);
                let _ = book.sync().await;
            }
        }
        self.clear_market_feed_state(&asset).await;

        Ok(())
    }

    pub async fn pause_all(&self) {
        for tx in self.markets.values() {
            let _ = tx.send(MarketCommand::Pause).await;
        }
    }

    pub async fn resume_all(&self) {
        for tx in self.markets.values() {
            let _ = tx.send(MarketCommand::Resume).await;
        }
    }

    pub async fn close_all(&mut self) {
        for (_asset, tx) in self.markets.drain() {
            let _ = tx.send(MarketCommand::Close).await;
        }
        self.market_price_routes.clear();
        self.market_required_assets.clear();
        self.asset_consumers.clear();
        let assets: Vec<_> = self.asset_feeds.keys().cloned().collect();
        for asset in assets {
            self.unsubscribe_asset_if_idle(asset).await;
        }
    }

    pub async fn send_cmd(&self, asset: String, cmd: MarketCommand) {
        if let Some(tx) = self.markets.get(&asset) {
            let tx = tx.clone();
            if let Err(e) = tx.send(cmd).await {
                log::warn!("Failed to send Market command: {:?}", e);
            }
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
        let mut book = margin_book.lock().await;
        match book.sync().await {
            Ok(_) => {
                let total = book.total_on_chain - book.used();
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
        if let Ok(fd) = usdc.parse::<f64>() {
            let cmd = MarketCommand::UserEvent(ExecEvent::Funding(fd));
            self.send_cmd(coin, cmd).await;
        } else {
            warn!("Failed to parse user funding");
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

        //SAFE
        let mut update_rv = self.update_rv.take().unwrap();
        //SAFE
        let mut price_router_rv = self.price_router_rv.take().unwrap();

        let session: Session = Arc::new(Mutex::new(HashMap::default()));

        let (margin_sync_reset_tx, mut margin_sync_reset_rx) = unbounded_channel::<()>();
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
        let margin_book_handle = tokio::spawn(async move {
            loop {
                let timer = sleep(Duration::from_secs(30));
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
                        if has_conn {
                            let _ = margin_tx.send(BotEvent::SyncMargin).await;
                        }
                    }
                }
            }
        });

        // MarketUpdate relay task — broadcasts to user via WsConnections
        let session_upd = session.clone();
        let upd_ws = ws_connections.clone();
        let upd_pk = pubkey.clone();
        let upd_pool = self.pool.clone();
        let upd_bot_tx = self._bot_tx.clone();

        tokio::spawn(async move {
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

                        let result = {
                            let mut book = margin_market_edit.lock().await;
                            book.update_asset(asset_margin.clone()).await
                        };

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

                                        // Persist to DB
                                        if let Some(ref pool) = upd_pool {
                                            let side_str = format!("{:?}", trade.side);
                                            let open_type = format!("{:?}", trade.open.fill_type);
                                            let close_type = format!("{:?}", trade.close.fill_type);
                                            if let Err(e) = sqlx::query(
                                                "INSERT INTO trades (pubkey, market, side, size, pnl, total_pnl, fees, funding, open_time, open_price, open_type, close_time, close_price, close_type, strategy) \
                                                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)"
                                            )
                                                .bind(&upd_pk)
                                                .bind(&asset)
                                                .bind(&side_str)
                                                .bind(trade.size)
                                                .bind(trade.pnl)
                                                .bind(trade.total_pnl)
                                                .bind(trade.fees)
                                                .bind(trade.funding)
                                                .bind(trade.open.time as i64)
                                                .bind(trade.open.price)
                                                .bind(&open_type)
                                                .bind(trade.close.time as i64)
                                                .bind(trade.close.price)
                                                .bind(&close_type)
                                                .bind(&trade.strategy)
                                                .execute(pool)
                                                .await
                                            {
                                                log::warn!("Failed to persist trade for {}: {:?}", asset, e);
                                            }
                                        }

                                        crate::EditMarketInfo::Trade(trade)
                                    }
                                }
                            } else {
                                edit
                            }
                        };
                        broadcast_to_user(&upd_ws, &upd_pk, MarketInfoEdit((asset, edit))).await;
                    }

                    M::RelayToFrontend(cmd) => {
                        broadcast_to_user(&upd_ws, &upd_pk, cmd).await;
                    }

                    M::AuthFailed(msg) => {
                        log::error!("[bot] auth failed: {msg} — notifying main loop");
                        let _ = upd_bot_tx.send(BotEvent::AuthFailed(msg)).await;
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
                        let _ = upd_bot_tx.send(BotEvent::RemoveMarket(asset)).await;
                    }
                }
            }
        });

        let (user_tx, mut user_rv) = unbounded_channel::<UserEventMessage>();
        let mut relay_user_events = false;

        if let Some(relay) = self.user_event_relay.clone() {
            match relay.subscribe(self.wallet.pubkey).await {
                Ok(mut quicknode_rx) => {
                    log::info!("Subscribed to shared QuickNode account event relay");
                    let user_tx = user_tx.clone();
                    tokio::spawn(async move {
                        while let Some(event) = quicknode_rx.recv().await {
                            if user_tx.send(UserEventMessage::QuickNode(event)).is_err() {
                                break;
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
            let _id = self
                .info_client
                .subscribe(
                    Subscription::UserEvents {
                        user: self.wallet.pubkey,
                    },
                    sdk_tx.clone(),
                )
                .await?;
            let _ledger_id = self
                .info_client
                .subscribe(
                    Subscription::UserNonFundingLedgerUpdates {
                        user: self.wallet.pubkey,
                    },
                    sdk_tx.clone(),
                )
                .await?;

            tokio::spawn(async move {
                while let Some(msg) = sdk_rx.recv().await {
                    if user_tx.send(UserEventMessage::Sdk(msg)).is_err() {
                        break;
                    }
                }
            });
        }

        loop {
            tokio::select!(
                biased;

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
                            AddMarket(_) | ResumeMarket(_) | ResumeAll | ManualUpdateMargin(_) | UpdateMarketStrategy(_) => {
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
                    match event {
                        AddMarket(add_market_info) => {
                            let asset = add_market_info.asset.clone();
                            if let Err(e) = self.add_market(add_market_info, &margin_user_edit).await {
                                self.send_to_frontend(UserError(format!("FAILED TO ADD MARKET: {}", e))).await;
                                self.send_to_frontend(CancelMarket(asset)).await;
                            }
                        }

                        ResumeMarket(asset) => {
                            let mut book = margin_user_edit.lock().await;
                            match book.sync().await {
                                Ok(positions) => {
                                    if positions.iter().any(|p| p.position.coin == asset) {
                                        drop(book);
                                        self.send_to_frontend(UserError(format!(
                                            "Cannot resume {}: close the on-chain position first",
                                            &asset
                                        ))).await;
                                        continue;
                                    }
                                }
                                Err(e) => {
                                    drop(book);
                                    self.send_to_frontend(UserError(format!(
                                        "Failed to check on-chain positions: {}", e
                                    ))).await;
                                    continue;
                                }
                            }
                            drop(book);
                            self.send_cmd(asset.clone(), MarketCommand::Resume).await;
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
                            self.send_cmd(asset.clone(), MarketCommand::Pause).await;
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
                            let _ = self.remove_market(asset.as_str(), &margin_user_edit).await;
                            let mut guard = session.lock().await;
                            let _ = guard.remove(&asset);
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
                                let _ = self.remove_market(market.as_str(), &margin_user_edit).await;
                                let mut guard = session.lock().await;
                                let _ = guard.remove(&market);
                            }
                        }

                        MarketComm(command) => {
                            if let MarketCommand::UpdateStrategy(_, _, ref name) = command.cmd {
                                let mut guard = session.lock().await;
                                if let Some(s) = guard.get_mut(&command.asset) {
                                    s.strategy_name = name.clone();
                                }
                            }else if let MarketCommand::UpdateLeverage(_lev) = command.cmd{
                                let mut guard = session.lock().await;
                                if let Some(s) = guard.get_mut(&command.asset)
                                    && s.position.is_some()
                                {
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
                                    let row = match sqlx::query_as::<_, crate::backend::db::StrategyRow>(
                                        "SELECT * FROM strategies WHERE id = $1",
                                    )
                                    .bind(sid)
                                    .fetch_optional(pool)
                                    .await
                                    {
                                        Ok(Some(r)) => r,
                                        Ok(None) => {
                                            self.send_to_frontend(UserError(format!("Strategy {} not found", sid))).await;
                                            continue;
                                        }
                                        Err(e) => {
                                            self.send_to_frontend(UserError(format!("DB error: {e}"))).await;
                                            continue;
                                        }
                                    };
                                    let state_decls: Option<crate::backend::scripting::StateDeclarations> = row
                                        .state_declarations
                                        .as_ref()
                                        .and_then(|v| serde_json::from_value(v.clone()).ok());
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
                                        serde_json::from_value(row.indicators).unwrap_or_default();
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

                            let mut guard = session.lock().await;
                            if let Some(s) = guard.get_mut(&asset) {
                                if matches!(s.engine_state, EngineView::Open | EngineView::Opening | EngineView::Closing){
                                    self.send_to_frontend(
                                        UserError(format!(
                                            "Margin update failed: {} market has open order(s)", &asset)
                                            )).await;
                                    continue;
                                }
                            let result = {
                                let mut book = margin_user_edit.lock().await;
                                book.update_asset(asset_margin.clone()).await
                            };

                            match result {
                                Ok(new_margin) => {
                                    {
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


                        }

                        SyncMargin => {
                            self.sync_margin_book(&margin_user_edit).await;
                        }

                        ReloadWallet(new_signer) => {
                            log::info!("[bot] reloading wallet for all {} markets", self.markets.len());
                            self.wallet = Arc::new(
                                Wallet::new(BaseUrl::Mainnet, self.wallet.pubkey, new_signer.clone()).await
                                    .expect("Wallet::new failed during hot-reload"),
                            );
                            for (asset, tx) in &self.markets {
                                if let Err(e) = tx.send(MarketCommand::ReloadWallet(new_signer.clone())).await {
                                    log::error!("[bot] failed to reload wallet for market {asset}: {e}");
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
                            for tx in self.markets.values() {
                                let _ = tx.send(MarketCommand::Pause).await;
                            }
                        }

                        ResumeAll => {
                            let mut book = margin_user_edit.lock().await;
                            let blocked: Vec<String> = match book.sync().await {
                                Ok(positions) => positions
                                    .iter()
                                    .filter(|p| self.markets.contains_key(&p.position.coin))
                                    .map(|p| p.position.coin.clone())
                                    .collect(),
                                Err(e) => {
                                    drop(book);
                                    self.send_to_frontend(UserError(format!(
                                        "Failed to check on-chain positions: {}", e
                                    ))).await;
                                    continue;
                                }
                            };
                            drop(book);

                            for (asset, tx) in self.markets.iter() {
                                if !blocked.contains(asset) {
                                    let _ = tx.send(MarketCommand::Resume).await;
                                }
                            }

                            let mut resumed: Vec<String> = Vec::new();
                            let mut guard = session.lock().await;
                            for (asset, s) in guard.iter_mut() {
                                if blocked.contains(asset) {
                                    continue;
                                }
                                s.is_paused = false;
                                resumed.push(asset.clone());
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
                        }

                        PauseAll => {
                            self.pause_all().await;
                            let assets: Vec<String> = self.markets.keys().cloned().collect();
                            let mut guard = session.lock().await;
                            for (_asset, s) in guard.iter_mut() {
                                s.is_paused = true;
                            }
                            drop(guard);
                            for asset in assets {
                                broadcast_to_user(&ws_connections, &pubkey, MarketInfoEdit((
                                    asset, crate::EditMarketInfo::Paused(true),
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
                        }

                        GetSession => {
                            let guard = session.lock().await;
                            let sess: Vec<MarketInfo> = guard.values().map(MarketInfo::from).collect();

                            let universe: Vec<AssetMeta> = match get_all_assets(&self.info_client).await {
                                Ok(u) => u,
                                Err(e) => {
                                    self.send_to_frontend(UserError(format!("Failed to fetch asset universe: {}", e))).await;
                                    Vec::new()
                                },
                            };

                            self.send_to_frontend(LoadSession((sess, universe))).await;
                        }

                        Kill => {
                            if let Some(relay) = &self.user_event_relay {
                                relay.unsubscribe(self.wallet.pubkey);
                            }
                            self.close_all().await;
                            {
                                let mut guard = session.lock().await;
                                guard.clear();
                            }
                            let mut book = margin_user_edit.lock().await;
                            book.reset();
                            let _ = self.info_client.shutdown_ws().await;
                            cancel_token.cancel();
                            let _ = margin_book_handle.await;
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
