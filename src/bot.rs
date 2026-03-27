use crate::{
    AddMarketInfo, BackendStatus, EngineView, ExecEvent, HLTradeInfo, Market, MarketCommand,
    MarketInfo, MarketState, MarketUpdate, TradeFillInfo, UpdateFrontend, Wallet,
};

use crate::backend::app_state::{StrategyCache, WsConnections, broadcast_to_user};
use crate::broadcast::{BroadcastCmd, CacheCmdIn, SubReply, SubscribePayload};
use hyperliquid_rust_sdk::{
    AssetMeta, AssetPosition, BaseUrl, Error, InfoClient, Message, Subscription, UserData,
};
use log::warn;
use rhai::Engine;
use rustc_hash::FxHasher;
use serde::Deserialize;
use sqlx::PgPool;
use std::collections::HashMap;
use std::hash::BuildHasherDefault;
use std::sync::Arc;
use tokio::sync::mpsc::{
    Receiver, Sender, UnboundedReceiver, UnboundedSender, channel, unbounded_channel,
};
use tokio::sync::{Mutex, oneshot};
use tokio::time::{Duration, interval};
use tokio_util::sync::CancellationToken;

use crate::helper::*;
use crate::margin::{AssetMargin, MarginBook};

pub type Session = Arc<Mutex<HashMap<String, MarketState, BuildHasherDefault<FxHasher>>>>;

pub struct Bot {
    info_client: InfoClient,
    wallet: Arc<Wallet>,
    markets: HashMap<String, Sender<MarketCommand>, BuildHasherDefault<FxHasher>>,
    broadcast_tx: UnboundedSender<BroadcastCmd>,
    candle_rx: Sender<CacheCmdIn>,
    #[allow(unused)]
    fees: (f64, f64),
    _bot_tx: Sender<BotEvent>,
    bot_rv: Receiver<BotEvent>,
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

impl Bot {
    pub async fn new(
        wallet: Wallet,
        broadcast_tx: UnboundedSender<BroadcastCmd>,
        candle_rx: Sender<CacheCmdIn>,
    ) -> Result<(Self, Sender<BotEvent>), Error> {
        let info_client = InfoClient::with_reconnect(None, Some(wallet.url)).await?;
        let fees = wallet.get_user_fees().await?;

        let (bot_tx, bot_rv) = channel::<BotEvent>(64);
        let (update_tx, update_rv) = unbounded_channel::<MarketUpdate>();

        Ok((
            Self {
                info_client,
                wallet: wallet.into(),
                markets: HashMap::default(),
                broadcast_tx,
                candle_rx,
                fees,
                _bot_tx: bot_tx.clone(),
                bot_rv,
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
            let guard = cache.read().await;
            let entry = guard
                .get(&sid)
                .ok_or_else(|| Error::Custom(format!("strategy {} not found in cache", sid)))?;
            (
                entry.compiled.clone(),
                entry.indicators.clone(),
                entry.name.clone(),
            )
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

        let (one_tx, one_rx) = oneshot::channel::<SubReply>();
        let sub_request = SubscribePayload {
            asset: asset.clone(),
            reply: one_tx,
        };

        self.broadcast_tx
            .send(BroadcastCmd::Subscribe(sub_request))
            .map_err(|e| Error::Custom(format!("broadcast channel closed: {}", e)))?;

        let sub_info = one_rx
            .await
            .map_err(|_| Error::Custom("subscription reply dropped".to_string()))??;

        let (market, market_tx) = Market::new(
            self.wallet.clone(),
            self.update_tx.clone(),
            self.candle_rx.clone(),
            sub_info.px_receiver,
            sub_info.meta,
            margin,
            lev,
            rhai_engine,
            compiled,
            strat_indicators,
            strategy_name,
            config,
        )
        .await?;

        self.markets.insert(asset.clone(), market_tx);

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

        let _ = self
            .broadcast_tx
            .send(BroadcastCmd::Unsubscribe(asset.clone()));

        if let Some(tx) = self.markets.remove(&asset) {
            let tx = tx.clone();
            let cmd = MarketCommand::Close;
            let close = tokio::spawn(async move {
                if let Err(e) = tx.send(cmd).await {
                    log::warn!("Failed to send Close command: {:?}", e);
                    return false;
                }
                true
            })
            .await
            .unwrap();

            if close {
                let mut book = margin_book.lock().await;
                book.remove(&asset);
            }
        }

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
        for (asset, tx) in self.markets.drain() {
            let _ = self.broadcast_tx.send(BroadcastCmd::Unsubscribe(asset));
            let _ = tx.send(MarketCommand::Close).await;
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

        let mut update_rv = self.update_rv.take().unwrap();

        let session: Session = Arc::new(Mutex::new(HashMap::default()));

        let user = self.wallet.clone();
        let margin_book = MarginBook::new(user);
        let margin_arc = Arc::new(Mutex::new(margin_book));
        let margin_user_edit = margin_arc.clone();
        let margin_market_edit = margin_arc.clone();
        let cancel_token = CancellationToken::new();

        // Margin sync fallback — sends SyncMargin every 30s only if user has active WS
        let margin_token = cancel_token.clone();
        let margin_tx = self._bot_tx.clone();
        let margin_ws = ws_connections.clone();
        let margin_pk = pubkey.clone();
        let margin_book_handle = tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(30));
            loop {
                tokio::select! {
                    _ = margin_token.cancelled() => { break; }
                    _ = ticker.tick() => {
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
                                        s.trades.push(trade.clone());

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
                }
            }
        });

        let (user_tx, mut user_rv) = unbounded_channel();
        let _id = self
            .info_client
            .subscribe(
                Subscription::UserEvents {
                    user: address(&self.wallet.pubkey),
                },
                user_tx,
            )
            .await?;

        loop {
            tokio::select!(
                biased;

                Some(msg) = user_rv.recv() => {
                    if let Message::User(user_event) = msg {
                        match user_event.data {
                            UserData::Fills(fills_vec) => {
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
                                                warn!("Failed to aggregate TradeFillInfo for {} market: {}", coin, e);
                                            }
                                        }
                                    }
                                }
                            }

                            UserData::Funding(funding_update) => {
                                if let Ok(fd) = funding_update.usdc.parse::<f64>() {
                                    let cmd = MarketCommand::UserEvent(ExecEvent::Funding(fd));
                                    self.send_cmd(funding_update.coin, cmd).await;
                                } else {
                                    warn!("Failed to parse user funding");
                                }
                            }

                            _ => { }
                        }
                    } else if let Message::NoData = msg {
                        self.send_to_frontend(Status(BackendStatus::Offline)).await;
                    }
                },

                Some(event) = self.bot_rv.recv() => {
                    // Block trading commands when key is invalid
                    if !self.key_valid {
                        match &event {
                            AddMarket(_) | ResumeMarket(_) | ResumeAll | ManualUpdateMargin(_) => {
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
                            let mut book = margin_user_edit.lock().await;
                            match book.sync().await {
                                Ok(_) => {
                                    let total = book.total_on_chain - book.used();
                                    self.send_to_frontend(UpdateTotalMargin(total)).await;
                                }
                                Err(e) => {
                                    warn!("Failed to fetch User Margin");
                                    self.send_to_frontend(UserError(format!(
                                        "Failed to fetch user margin: {e}"
                                    ))).await;
                                }
                            }
                        }

                        ReloadWallet(new_signer) => {
                            log::info!("[bot] reloading wallet for all {} markets", self.markets.len());
                            self.wallet = Arc::new(
                                Wallet::new(BaseUrl::Mainnet, self.wallet.pubkey.clone(), new_signer.clone()).await
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

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BotEvent {
    AddMarket(AddMarketInfo),
    ResumeMarket(String),
    PauseMarket(String),
    RemoveMarket(String),
    MarketComm(BotToMarket),
    ManualUpdateMargin(AssetMargin),
    SyncMargin,
    #[serde(skip)]
    ReloadWallet(alloy::signers::local::PrivateKeySigner),
    #[serde(skip)]
    AuthFailed(String),
    ResumeAll,
    PauseAll,
    CloseAll,
    GetSession,
    Kill,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BotToMarket {
    pub asset: String,
    pub cmd: MarketCommand,
}
