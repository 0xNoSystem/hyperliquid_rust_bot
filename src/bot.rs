use crate::{
    AddMarketInfo, BackendStatus, ExecEvent, HLTradeInfo, Market, MarketCommand, MarketInfo,
    MarketState, MarketUpdate, TradeFillInfo, UpdateFrontend, Wallet,
};
use hyperliquid_rust_sdk::{
    AssetMeta, AssetPosition, Error, InfoClient, Message, Subscription, UserData,
};
use log::{info, warn};
use rustc_hash::FxHasher;
use serde::Deserialize;
use std::collections::HashMap;
use std::hash::BuildHasherDefault;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::mpsc::{
    Receiver, Sender, UnboundedReceiver, UnboundedSender, channel, unbounded_channel,
};
use tokio::time::{Duration, interval};
use tokio_util::sync::CancellationToken;

use crate::helper::*;
use crate::margin::{AssetMargin, MarginBook};

pub type Session = Arc<Mutex<HashMap<String, MarketState, BuildHasherDefault<FxHasher>>>>;

pub struct Bot {
    info_client: InfoClient,
    wallet: Arc<Wallet>,
    markets: HashMap<String, Sender<MarketCommand>, BuildHasherDefault<FxHasher>>,
    candle_subs: HashMap<String, u32>,
    #[allow(unused)]
    fees: (f64, f64),
    _bot_tx: Sender<BotEvent>,
    bot_rv: Receiver<BotEvent>,
    update_rv: Option<UnboundedReceiver<MarketUpdate>>,
    update_tx: UnboundedSender<MarketUpdate>,
    app_tx: Option<Sender<UpdateFrontend>>,
    chain_open_positions: Vec<AssetPosition>,
    universe: Vec<AssetMeta>,
}

impl Bot {
    pub async fn new(wallet: Wallet) -> Result<(Self, Sender<BotEvent>), Error> {
        let info_client = InfoClient::with_reconnect(None, Some(wallet.url)).await?;
        let fees = wallet.get_user_fees().await?;
        let universe = get_all_assets(&info_client).await?;

        let (bot_tx, bot_rv) = channel::<BotEvent>(64);
        let (update_tx, update_rv) = unbounded_channel::<MarketUpdate>();

        Ok((
            Self {
                info_client,
                wallet: wallet.into(),
                markets: HashMap::default(),
                candle_subs: HashMap::new(),
                fees,
                _bot_tx: bot_tx.clone(),
                bot_rv,
                update_rv: Some(update_rv),
                update_tx,
                app_tx: None,
                chain_open_positions: Vec::new(),
                universe,
            },
            bot_tx,
        ))
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
            strategy,
            config,
        } = info;

        let asset = asset.trim().to_string();
        let asset_str = asset.as_str();

        if self.markets.contains_key(&asset) {
            if let Some(tx) = &self.app_tx {
                let _ = tx.try_send(UpdateFrontend::UserError(format!(
                    "{} market is already added.",
                    &asset
                )));
            }
            return Ok(());
        }

        let mut book = margin_book.lock().await;
        self.chain_open_positions = book.sync().await?;
        if self
            .chain_open_positions
            .iter()
            .any(|p| p.position.coin == asset)
        {
            if let Some(tx) = &self.app_tx {
                let _ = tx.try_send(UpdateFrontend::UserError(format!(
                    "Cannot add a market with open on-chain position({})",
                    &asset
                )));
            }
            return Ok(());
        }

        let margin = book.allocate(asset.clone(), margin_alloc).await?;
        drop(book);

        if let Some(tx) = &self.app_tx {
            let _ = tx
                .send(UpdateFrontend::PreconfirmMarket(asset.clone()))
                .await;
        }

        let meta = if let Some(cached) = self.universe.iter().find(|a| a.name == asset_str).cloned()
        {
            cached
        } else {
            get_asset(&self.info_client, asset_str).await?
        };

        let (sub_id, receiver) = subscribe_candles(&mut self.info_client, asset_str).await?;

        let (market, market_tx) = Market::new(
            self.wallet.clone(),
            self.update_tx.clone(),
            receiver,
            meta,
            margin,
            lev,
            strategy,
            config,
        )
        .await?;

        self.markets.insert(asset.clone(), market_tx);
        self.candle_subs.insert(asset.clone(), sub_id);

        let app_tx = self.app_tx.clone();
        let remove_market_tx = self._bot_tx.clone();

        tokio::spawn(async move {
            if let Err(e) = market.start().await {
                if let Some(tx) = app_tx {
                    let _ = tx.send(UpdateFrontend::CancelMarket(asset.clone())).await;
                    let _ = tx
                        .send(UpdateFrontend::UserError(format!(
                            "Market {} exited with error:\n {:?}",
                            &asset, e
                        )))
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

        if let Some(sub_id) = self.candle_subs.remove(&asset) {
            self.info_client.unsubscribe(sub_id).await?;
            info!("Removed {} market successfully", asset);
        } else {
            info!("Couldn't remove {} market, it doesn't exist", asset);
            return Ok(());
        }

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
        } else {
            info!("Failed: Close {} market, it doesn't exist", asset);
        }

        Ok(())
    }

    pub async fn pause_all(&self) {
        info!("PAUSING ALL MARKETS");
        for tx in self.markets.values() {
            let _ = tx.send(MarketCommand::Pause).await;
        }
    }

    pub async fn resume_all(&self) {
        info!("RESUMING ALL MARKETS");
        for tx in self.markets.values() {
            let _ = tx.send(MarketCommand::Resume).await;
        }
    }

    pub async fn close_all(&mut self) {
        info!("CLOSING ALL MARKETS");
        for (_asset, id) in self.candle_subs.drain() {
            let _ = self.info_client.unsubscribe(id).await;
        }
        self.candle_subs.clear();

        for (_asset, tx) in self.markets.drain() {
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

    pub async fn start(mut self, app_tx: Sender<UpdateFrontend>) -> Result<(), Error> {
        use BotEvent::*;
        use MarketUpdate as M;
        use UpdateFrontend::*;

        self.app_tx = Some(app_tx.clone());

        let mut update_rv = self.update_rv.take().unwrap();

        let session: Session = Arc::new(Mutex::new(HashMap::default()));

        let user = self.wallet.clone();
        let margin_book = MarginBook::new(user);
        let margin_arc = Arc::new(Mutex::new(margin_book));
        let margin_sync = margin_arc.clone();
        let margin_user_edit = margin_arc.clone();
        let margin_market_edit = margin_arc.clone();
        let cancel_token = CancellationToken::new();

        let app_tx_margin = app_tx.clone();
        let err_tx = app_tx.clone();

        let margin_token = cancel_token.clone();
        let margin_book_handle = tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(2));
            loop {
                tokio::select! {
                    _ = margin_token.cancelled() => { break; }
                    _ = ticker.tick() => {
                        let mut book = margin_sync.lock().await;
                        let result = book.sync().await;

                        match result {
                            Ok(_) => {
                                let total = book.total_on_chain - book.used();
                                let _ = app_tx_margin.try_send(UpdateTotalMargin(total));
                            }
                            Err(e) => {
                                warn!("Failed to fetch User Margin");
                                let _ = app_tx_margin.try_send(UserError(format!(
                                    "Failed to fetch user margin, check your connection: {e}"
                                )));
                            }
                        }
                    }
                }
            }
        });

        let session_upd = session.clone();
        let app_tx_upd = app_tx.clone();

        tokio::spawn(async move {
            while let Some(market_update) = update_rv.recv().await {
                match market_update {
                    M::InitMarket(info) => {
                        let state = MarketState::from(&info);
                        {
                            let mut guard = session_upd.lock().await;
                            guard.insert(info.asset.clone(), state);
                        }
                        let _ = app_tx_upd.try_send(ConfirmMarket(info));
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
                                let _ = app_tx_upd.try_send(UpdateMarketMargin(asset_margin));
                            }
                            Err(e) => {
                                let _ = app_tx_upd.try_send(UserError(e.to_string()));
                            }
                        }
                    }

                    M::MarketInfoUpdate((asset, edit)) => {
                        {
                            let mut guard = session_upd.lock().await;
                            if let Some(s) = guard.get_mut(&asset) {
                                match edit {
                                    crate::EditMarketInfo::Lev(lev) => {
                                        s.lev = lev;
                                    }
                                    crate::EditMarketInfo::OpenPosition(pos) => {
                                        s.position = pos;
                                    }
                                    crate::EditMarketInfo::EngineState(view) => {
                                        s.engine_state = view;
                                    }
                                    crate::EditMarketInfo::Trade(trade) => {
                                        s.pnl += trade.pnl;
                                        s.trades.push(trade);
                                    }
                                }
                            }
                        }
                        let _ = app_tx_upd.try_send(MarketInfoEdit((asset, edit)));
                    }

                    M::RelayToFrontend(cmd) => {
                        let _ = app_tx_upd.try_send(cmd);
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

                            _ => { info!("{:?}", user_event) }
                        }
                    } else if let Message::NoData = msg {
                        info!("Received Message::NoData from WS, check connection");
                        let _ = err_tx.send(Status(BackendStatus::Offline)).await;
                    }
                },

                Some(event) = self.bot_rv.recv() => {
                    match event {
                        AddMarket(add_market_info) => {
                            let asset = add_market_info.asset.clone();
                            if let Err(e) = self.add_market(add_market_info, &margin_user_edit).await {
                                let _ = err_tx.try_send(UserError(format!("FAILED TO ADD MARKET: {}", e)));
                                let _ = err_tx.send(CancelMarket(asset)).await;
                            }
                        }

                        ResumeMarket(asset) => {
                            self.send_cmd(asset.clone(), MarketCommand::Resume).await;
                            let mut guard = session.lock().await;
                            if let Some(s) = guard.get_mut(&asset) {
                                s.is_paused = false;
                            }
                        }

                        PauseMarket(asset) => {
                            self.send_cmd(asset.clone(), MarketCommand::Pause).await;
                            let mut guard = session.lock().await;
                            if let Some(s) = guard.get_mut(&asset) {
                                s.is_paused = true;
                            }
                        }

                        RemoveMarket(asset) => {
                            let _ = self.remove_market(asset.as_str(), &margin_user_edit).await;
                            let mut guard = session.lock().await;
                            let _ = guard.remove(&asset);
                        }

                        MarketComm(command) => {
                            if let MarketCommand::UpdateStrategy(strat) = command.cmd{
                                let mut guard = session.lock().await;
                                if let Some(s) = guard.get_mut(&command.asset){
                                    s.strategy = strat;
                                }
                            }
                            self.send_cmd(command.asset, command.cmd).await;
                        }

                        ManualUpdateMargin(asset_margin) => {
                            let asset = asset_margin.0.clone();

                            let result = {
                                let mut book = margin_user_edit.lock().await;
                                book.update_asset(asset_margin.clone()).await
                            };

                            match result {
                                Ok(new_margin) => {
                                    {
                                        let mut guard = session.lock().await;
                                        if let Some(s) = guard.get_mut(&asset) {
                                            s.margin = new_margin;
                                        }
                                    }
                                    let _ = err_tx.try_send(UpdateMarketMargin((asset.clone(), new_margin)));
                                    let cmd = MarketCommand::UpdateMargin(new_margin);
                                    self.send_cmd(asset, cmd).await;
                                }

                                Err(e) => {
                                    let _ = err_tx.try_send(UserError(e.to_string()));
                                }
                            }
                        }

                        ResumeAll => {
                            self.resume_all().await;
                            let mut guard = session.lock().await;
                            for (_asset, s) in guard.iter_mut() {
                                s.is_paused = false;
                            }
                        }

                        PauseAll => {
                            self.pause_all().await;
                            let mut guard = session.lock().await;
                            for (_asset, s) in guard.iter_mut() {
                                s.is_paused = true;
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

                            let universe: Vec<AssetMeta> = if self.universe.is_empty() {
                                match get_all_assets(&self.info_client).await {
                                    Ok(u) => u,
                                    Err(e) => {
                                        let _ = err_tx.try_send(UserError(format!("Failed to fetch asset universe: {}", e)));
                                        Vec::new()
                                    },
                                }
                            } else {
                                self.universe.clone()
                            };

                            let _ = err_tx.try_send(LoadSession((sess, universe)));
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
