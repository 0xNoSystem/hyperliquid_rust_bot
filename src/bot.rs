use crate::{
    AddMarketInfo, ExecEvent, HLTradeInfo, Market, MarketCommand, MarketInfo, MarketUpdate,
    TradeFillInfo, UpdateFrontend, Wallet,
};
use hyperliquid_rust_sdk::{
    AssetMeta, AssetPosition, Error, InfoClient, Message, Subscription, UserData,
};
use log::{info, warn};
use std::collections::HashMap;
use tokio::time::{Duration, interval, sleep};

use crate::helper::*;
use tokio::sync::mpsc::{Sender, UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::helper::address;
use crate::margin::{AssetMargin, MarginBook};
use std::sync::Arc;
use tokio::sync::Mutex;

use rustc_hash::FxHasher;
use serde::Deserialize;
use std::hash::BuildHasherDefault;

pub struct Bot {
    info_client: InfoClient,
    wallet: Arc<Wallet>,
    markets: HashMap<String, Sender<MarketCommand>, BuildHasherDefault<FxHasher>>,
    candle_subs: HashMap<String, u32>,
    session: Arc<Mutex<HashMap<String, MarketInfo, BuildHasherDefault<FxHasher>>>>,
    fees: (f64, f64),
    _bot_tx: UnboundedSender<BotEvent>,
    bot_rv: UnboundedReceiver<BotEvent>,
    update_rv: Option<UnboundedReceiver<MarketUpdate>>,
    update_tx: UnboundedSender<MarketUpdate>,
    app_tx: Option<UnboundedSender<UpdateFrontend>>,
    chain_open_positions: Vec<AssetPosition>,
    universe: Vec<AssetMeta>,
}

impl Bot {
    pub async fn new(wallet: Wallet) -> Result<(Self, UnboundedSender<BotEvent>), Error> {
        let info_client = InfoClient::with_reconnect(None, Some(wallet.url)).await?;
        let fees = wallet.get_user_fees().await?;
        let universe = get_all_assets(&info_client).await?;

        let (bot_tx, bot_rv) = unbounded_channel::<BotEvent>();
        let (update_tx, update_rv) = unbounded_channel::<MarketUpdate>();

        Ok((
            Self {
                info_client,
                wallet: wallet.into(),
                markets: HashMap::default(),
                candle_subs: HashMap::new(),
                session: Arc::new(Mutex::new(HashMap::default())),
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
            trade_params,
            config,
        } = info;
        let asset = asset.trim().to_string();
        let asset_str = asset.as_str();

        if self.markets.contains_key(&asset) {
            if let Some(tx) = &self.app_tx {
                let _ = tx.send(UpdateFrontend::UserError(format!(
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
                let _ = tx.send(UpdateFrontend::UserError(format!(
                    "Cannot add a market with open on-chain position({})",
                    &asset
                )));
            }
            return Ok(());
        }

        let margin = book.allocate(asset.clone(), margin_alloc).await?;

        if let Some(tx) = &self.app_tx {
            let _ = tx.send(UpdateFrontend::PreconfirmMarket(asset.clone()));
        }

        let meta = if let Some(cached) = self.universe.iter().find(|a| a.name == asset_str).cloned()
        {
            cached
        } else {
            get_asset(&self.info_client, asset_str).await?
        };

        let (sub_id, receiver) = subscribe_candles(
            &mut self.info_client,
            asset_str,
            trade_params.time_frame.as_str(),
        )
        .await?;

        let (market, market_tx) = Market::new(
            self.wallet.clone(),
            self.update_tx.clone(),
            receiver,
            meta,
            margin,
            self.fees,
            trade_params,
            config,
        )
        .await?;

        self.markets.insert(asset.clone(), market_tx);
        self.candle_subs.insert(asset.clone(), sub_id);
        let cancel_margin = margin_book.clone();
        let app_tx = self.app_tx.clone();

        tokio::spawn(async move {
            if let Err(e) = market.start().await {
                if let Some(tx) = app_tx {
                    let _ = tx.send(UpdateFrontend::UserError(format!(
                        "Market {} exited with error: {:?}",
                        &asset, e
                    )));
                }
                let mut book = cancel_margin.lock().await;
                book.remove(&asset);
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
                let mut sess_guard = self.session.lock().await;
                let _ = sess_guard.remove(&asset);
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

        let mut session = self.session.lock().await;
        for (_asset, info) in session.iter_mut() {
            info.is_paused = true;
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

        let mut session = self.session.lock().await;
        session.clear();
    }

    pub async fn send_cmd(&self, asset: String, cmd: MarketCommand) {
        if let Some(tx) = self.markets.get(&asset) {
            let tx = tx.clone();
            tokio::spawn(async move {
                if let Err(e) = tx.send(cmd).await {
                    log::warn!("Failed to send Market command: {:?}", e);
                }
            });
        }
    }

    pub fn get_markets(&self) -> Vec<&String> {
        let mut assets = Vec::new();
        for asset in self.markets.keys() {
            assets.push(asset);
        }

        assets
    }

    pub async fn get_session(&self) -> Result<(Vec<MarketInfo>, Vec<AssetMeta>), Error> {
        let guard = self.session.lock().await;
        let session: Vec<MarketInfo> = guard.values().cloned().collect();

        let universe: Vec<AssetMeta> = if self.universe.is_empty() {
            get_all_assets(&self.info_client).await?
        } else {
            self.universe.clone()
        };

        Ok((session, universe))
    }

    pub async fn start(mut self, app_tx: UnboundedSender<UpdateFrontend>) -> Result<(), Error> {
        use BotEvent::*;
        use MarketUpdate::*;
        use UpdateFrontend::*;

        self.app_tx = Some(app_tx.clone());

        let mut update_rv = self.update_rv.take().unwrap();

        let user = self.wallet.clone();
        let margin_book = MarginBook::new(user);
        let margin_arc = Arc::new(Mutex::new(margin_book));
        let margin_sync = margin_arc.clone();
        let margin_user_edit = margin_arc.clone();
        let margin_market_edit = margin_arc.clone();

        let app_tx_margin = app_tx.clone();
        let err_tx = app_tx.clone();

        //keep marginbook in sync for DEX <=> BOT overlap
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(2));
            loop {
                ticker.tick().await;
                let result = {
                    let mut book = margin_sync.lock().await;
                    book.sync().await
                };

                match result {
                    Ok(_) => {
                        let total = {
                            let book = margin_sync.lock().await;
                            book.total_on_chain - book.used()
                        };
                        let _ = app_tx_margin.send(UpdateTotalMargin(total));
                    }
                    Err(e) => {
                        log::warn!("Failed to fetch User Margin");
                        let _ = app_tx_margin.send(UserError(e.to_string()));
                        continue;
                    }
                }
            }
        });

        //Market -> Bot
        let session_adder = self.session.clone();
        tokio::spawn(async move {
            while let Some(market_update) = update_rv.recv().await {
                match market_update {
                    InitMarket(info) => {
                        let mut session_guard = session_adder.lock().await;
                        session_guard.insert(info.asset.clone(), info.clone());
                        let _ = app_tx.send(ConfirmMarket(info));
                    }
                    PriceUpdate(asset_price) => {
                        let _ = app_tx.send(UpdatePrice(asset_price));
                    }
                    TradeUpdate(trade_info) => {
                        let _ = app_tx.send(NewTradeInfo(trade_info));
                    }
                    MarginUpdate(asset_margin) => {
                        let result = {
                            let mut book = margin_market_edit.lock().await;
                            book.update_asset(asset_margin.clone()).await
                        };

                        match result {
                            Ok(_) => {
                                let _ = app_tx.send(UpdateMarketMargin(asset_margin));
                            }
                            Err(e) => {
                                let _ = app_tx.send(UserError(e.to_string()));
                            }
                        }
                    }
                    RelayToFrontend(cmd) => {
                        let _ = app_tx.send(cmd);
                    }
                }
            }
        });

        //fill events
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

                    Some(Message::User(user_event)) = user_rv.recv() => {

                    match user_event.data{

                        UserData::Fills(fills_vec) =>{

                        let mut fills_map: HashMap<
                            String,
                            HashMap<
                                u64,
                                Vec<HLTradeInfo>,
                                BuildHasherDefault<FxHasher>
                            >,
                            BuildHasherDefault<FxHasher>> = HashMap::default();

                        for trade in fills_vec.into_iter(){
                            let coin = trade.coin.clone();
                            let oid = trade.oid;
                            fills_map
                                .entry(coin)
                                .or_default()
                                .entry(oid)
                                .or_default()
                                .push(trade);
                        }
                        println!("\nTRADES  |||||||||| {:?}\n\n", fills_map);

                        for (coin, map) in fills_map.into_iter()
                        {
                            for (_oid, fills) in map.into_iter() {
                                match TradeFillInfo::try_from(fills) {
                                    Ok(fill) => {
                                        let cmd = MarketCommand::UserEvent(ExecEvent::Fill(fill));
                                        let _ = sleep(Duration::from_millis(1000));
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
                        if let Ok(fd) = funding_update.usdc.parse::<f64>(){
                            let cmd = MarketCommand::UserEvent(ExecEvent::Funding(fd));
                            self.send_cmd(funding_update.coin, cmd).await;
                        }else{
                            warn!("Failed to parse user funding");
                        }
                    }
                    _ => info!("{:?}", user_event)
                }
            },


                    Some(event) = self.bot_rv.recv() => {

                        match event{
                            AddMarket(add_market_info) => {
                                if let Err(e) = self.add_market(add_market_info, &margin_user_edit).await{
                                    let _ = err_tx.send(UserError(e.to_string()));
                            }
                        },
                            ResumeMarket(asset) => {
                                self.send_cmd(asset.clone(), MarketCommand::Resume).await;
                                let mut sess_guard = self.session.lock().await;
                                if let Some(info) = sess_guard.get_mut(&asset) {
                                    info.is_paused = false;
                                }
                            },
                            PauseMarket(asset) => {
                                self.send_cmd(asset.clone(), MarketCommand::Pause).await;
                                let mut sess_guard = self.session.lock().await;
                                if let Some(info) = sess_guard.get_mut(&asset) {
                                    info.is_paused = true;
                                }
                            },
                            RemoveMarket(asset) => {let _ = self.remove_market(asset.as_str(), &margin_user_edit).await;},
                            MarketComm(command) => {self.send_cmd(command.asset, command.cmd).await;},
                            ManualUpdateMargin(asset_margin) => {
                                let result = {
                                    let mut book = margin_user_edit.lock().await;
                                    book.update_asset(asset_margin.clone()).await
                                };
                                match result{
                                Ok(new_margin) => {
                                    let cmd = MarketCommand::UpdateMargin(new_margin);
                                    self.send_cmd(asset_margin.0, cmd).await;
                                },

                                Err(e) => {
                                    let _ = err_tx.send(UserError(e.to_string()));
                                },
                                }

                            },
                            ResumeAll =>{self.resume_all().await},
                            PauseAll => {self.pause_all().await;},
                            CloseAll => {
                                self.close_all().await;
                                let mut book = margin_user_edit.lock().await;
                                book.reset();
                            },

                            GetSession =>{
                                let session = self.get_session().await;
                                if let Ok(session) = session{
                                    let _ = err_tx.send(LoadSession(session));
                                }else{
                                    let _ = err_tx.send(UserError("Failed to load session from server.".to_string()));
                                }
                            },
                        }
                },


            )
        }
    }
}

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
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BotToMarket {
    pub asset: String,
    pub cmd: MarketCommand,
}
