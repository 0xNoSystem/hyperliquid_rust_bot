#![allow(unused_variables)]
use log::{info, warn};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use hyperliquid_rust_sdk::{AssetMeta, Error, ExchangeClient, InfoClient, Message};

use crate::signal::{
    EditType, EngineCommand, Entry, ExecParam, ExecParams, IndexId, SignalEngine, TimeFrameData,
};
use crate::{AssetMargin, EditMarketInfo, IndicatorData, UpdateFrontend};
use crate::{
    ExecCommand, ExecControl, ExecEvent, Executor, candles_snapshot, get_time_now, load_candles,
    parse_candle,
};
use crate::{MAX_DISCONNECTION_WINDOW, MAX_HISTORY, MarketInfo, Strategy, Wallet};

use crate::{OpenPositionLocal, TimeFrame, TradeInfo, TradeParams};

use tokio::sync::mpsc::{
    Receiver, Sender, UnboundedReceiver, UnboundedSender, channel, unbounded_channel,
};
use tokio::task::JoinHandle;

use flume::{Sender as FlumeSender, bounded};

pub struct Market {
    info_client: InfoClient,
    exchange_client: ExchangeClient,
    pub trade_history: Vec<TradeInfo>,
    pub pnl: f64,
    pub trade_params: TradeParams,
    pub asset: AssetMeta,
    signal_engine: SignalEngine,
    executor: Executor,
    receivers: MarketReceivers,
    senders: MarketSenders,
    pub active_tfs: HashSet<TimeFrame>,
    pub margin: f64,
    disconnected: Arc<AtomicBool>,
}

impl Market {
    pub async fn new(
        wallet: Arc<Wallet>,
        bot_tx: UnboundedSender<MarketUpdate>,
        price_rv: UnboundedReceiver<Message>,
        asset: AssetMeta,
        margin: f64,
        trade_params: TradeParams,
        config: Option<Vec<IndexId>>,
    ) -> Result<(Self, Sender<MarketCommand>), Error> {
        info!("{:?}", &asset);
        let info_client = InfoClient::new(None, Some(wallet.url)).await?;
        let exchange_client =
            ExchangeClient::new(None, wallet.wallet.clone(), Some(wallet.url), None, None).await?;

        //Look up needed tfs for loading
        let strat_indicators = trade_params.strategy.indicators();
        let mut active_tfs: HashSet<TimeFrame> =
            strat_indicators.into_iter().map(|id| id.1).collect();
        if let Some(ref cfg) = config {
            for ind_id in cfg {
                active_tfs.insert(ind_id.1);
            }
        }

        //setup channels
        let (market_tx, market_rv) = channel::<MarketCommand>(7);
        let (exec_tx, exec_rv) = bounded::<ExecCommand>(2);
        let (engine_tx, engine_rv) = unbounded_channel::<EngineCommand>();

        let senders = MarketSenders {
            bot_tx,
            engine_tx,
            exec_tx: exec_tx.clone(),
            market_tx: market_tx.clone(),
        };

        let receivers = MarketReceivers {
            price_rv,
            market_rv,
        };

        let lev = trade_params.lev.min(asset.max_leverage);
        let exec_params = ExecParams::new(margin, lev);

        Ok((
            Market {
                info_client,
                exchange_client,
                margin,
                trade_history: Vec::with_capacity(MAX_HISTORY),
                pnl: 0_f64,
                trade_params: trade_params.clone(),
                asset: asset.clone(),
                signal_engine: SignalEngine::new(
                    config,
                    trade_params,
                    engine_rv,
                    Some(market_tx.clone()),
                    exec_tx,
                    exec_params,
                )
                .await,
                executor: Executor::new(wallet.wallet.clone(), asset, exec_rv, market_tx.clone())
                    .await?,
                receivers,
                senders,
                active_tfs,
                disconnected: Arc::new(AtomicBool::new(false)),
            },
            market_tx,
        ))
    }

    async fn init(&mut self) -> Result<(), Error> {
        //check if lev > max_lev
        let lev = self.trade_params.lev.min(self.asset.max_leverage);
        let upd = self
            .trade_params
            .update_lev(lev, &self.exchange_client, self.asset.name.as_str(), true)
            .await;
        if let Ok(lev) = upd {
            let engine_tx = self.senders.engine_tx.clone();
            let _ = engine_tx.send(EngineCommand::UpdateExecParams(ExecParam::Lev(lev)));
        };

        self.load_engine(2000).await?;
        println!(
            "\nMarket initialized for {} {:?}\n",
            self.asset.name, self.trade_params
        );
        Ok(())
    }

    pub fn change_strategy(&mut self, strategy: Strategy) {
        self.trade_params.strategy = strategy;
    }

    async fn load_engine(&mut self, candle_count: u64) -> Result<(), Error> {
        info!("---------------Loading Engine---------------");
        for tf in &self.active_tfs {
            let price_data = load_candles(
                &self.info_client,
                self.asset.name.as_str(),
                *tf,
                candle_count,
            )
            .await?;
            self.signal_engine.load(*tf, price_data).await;
        }

        Ok(())
    }

    pub fn get_trade_history(&self) -> &Vec<TradeInfo> {
        &self.trade_history
    }
}

impl Market {
    pub async fn start(mut self) -> Result<(), Error> {
        use ExecCommand::*;
        self.init().await?;

        let info = MarketInfo {
            asset: self.asset.name.clone(),
            lev: self.trade_params.lev,
            price: 0.0,
            params: self.trade_params.clone(),
            margin: self.margin,
            pnl: 0.0,
            is_paused: false,
            indicators: self.signal_engine.get_indicators_data(),
            position: None,
        };
        let _ = self.senders.bot_tx.send(MarketUpdate::InitMarket(info));

        let mut signal_engine = self.signal_engine;
        let mut executor = self.executor;

        //Start engine
        let engine_handle = tokio::spawn(async move {
            signal_engine.start().await;
        });
        //Start exucutor
        let executor_handle = tokio::spawn(async move {
            executor.start().await;
        });
        //Candle Stream
        let engine_price_tx = self.senders.engine_tx.clone();
        let bot_price_update = self.senders.bot_tx.clone();

        let asset_name: Arc<str> = Arc::from(self.asset.name.clone());
        let disconnected_price_stream_flag = self.disconnected.clone();
        let disconnection_info_sender = self.senders.market_tx;

        let candle_stream_handle: JoinHandle<Result<(), Error>> = tokio::spawn(async move {
            let mut disconnected = false;
            let mut disconnection_start: Option<Instant> = None;
            let mut last_confirmed_close: Option<u64> = None;
            while let Some(msg) = self.receivers.price_rv.recv().await {
                match msg {
                    Message::Candle(candle) => {
                        let price = parse_candle(candle.data)?;
                        let _ = bot_price_update.send(MarketUpdate::RelayToFrontend(
                            UpdateFrontend::MarketInfoEdit((
                                asset_name.clone().to_string(),
                                EditMarketInfo::Price(price.close),
                            )),
                        ));
                        if disconnected {
                            if disconnected_price_stream_flag.load(Ordering::Relaxed) {
                                let mut send_next_px = false;
                                //Check if missed window is worth fetching
                                if let Some(timer) = disconnection_start.take() {
                                    if timer.elapsed().as_millis() > MAX_DISCONNECTION_WINDOW {
                                        let cmd = MarketCommand::FetchMissedWindow(
                                            last_confirmed_close.unwrap_or_else(get_time_now),
                                        );
                                        if let Err(e) = disconnection_info_sender.send(cmd).await {
                                            warn!(
                                                "Failed to notify market about missing price window,
                                                    close the market and reopen if indicators drift: {}",e);
                                            disconnected_price_stream_flag
                                                .store(false, Ordering::Relaxed);
                                            disconnected = false;
                                        }
                                        continue;
                                    } else {
                                        disconnected_price_stream_flag
                                            .store(false, Ordering::Relaxed);
                                        disconnected = false;
                                        send_next_px = true;
                                    }
                                }
                                if !send_next_px {
                                    continue;
                                }
                            } else {
                                disconnected = false;
                            }
                        }
                        last_confirmed_close = Some(price.open_time);
                        let _ = engine_price_tx.send(EngineCommand::UpdatePrice(price));
                    }
                    Message::NoData => {
                        if !disconnected {
                            disconnected_price_stream_flag.store(true, Ordering::Relaxed);
                            disconnected = true;
                            disconnection_start = Some(Instant::now());
                            info!(
                                "{} price stream disconnected, receiver is still alive and well",
                                &asset_name
                            );
                        }
                    }
                    _ => {}
                }
            }
            Ok(())
        });

        //listen to changes and trade results
        let engine_update_tx = self.senders.engine_tx.clone();
        let bot_update_tx = self.senders.bot_tx;
        let asset = self.asset.clone();

        let disconnection_flag = self.disconnected.clone();
        while let Some(cmd) = self.receivers.market_rv.recv().await {
            match cmd {
                MarketCommand::UpdateLeverage(lev) => {
                    let lev = lev.min(asset.max_leverage);
                    let upd = self
                        .trade_params
                        .update_lev(lev, &self.exchange_client, asset.name.as_str(), false)
                        .await;
                    match upd {
                        Ok(lev) => {
                            let _ = engine_update_tx
                                .send(EngineCommand::UpdateExecParams(ExecParam::Lev(lev)));
                            let _ = bot_update_tx.send(MarketUpdate::RelayToFrontend(
                                UpdateFrontend::MarketInfoEdit((
                                    asset.name.clone(),
                                    EditMarketInfo::Lev(lev),
                                )),
                            ));
                        }
                        Err(e) => {
                            let _ = bot_update_tx.send(MarketUpdate::RelayToFrontend(
                                UpdateFrontend::UserError(e.to_string()),
                            ));
                        }
                    }
                }

                MarketCommand::UpdateStrategy(strat) => {
                    //let _ = engine_update_tx.send(EngineCommand::UpdateStrategy(strat));
                }

                MarketCommand::EditIndicators(entry_vec) => {
                    let mut map: TimeFrameData = HashMap::default();
                    for &entry in &entry_vec {
                        if entry.edit == EditType::Add && !self.active_tfs.contains(&entry.id.1) {
                            let tf_data = load_candles(
                                &self.info_client,
                                asset.name.as_str(),
                                entry.id.1,
                                5000,
                            )
                            .await?;
                            map.insert(entry.id.1, tf_data);
                            self.active_tfs.insert(entry.id.1);
                        }
                    }

                    let price_data = if map.is_empty() { None } else { Some(map) };
                    let _ = engine_update_tx.send(EngineCommand::EditIndicators {
                        indicators: entry_vec,
                        price_data,
                    });
                }

                MarketCommand::UpdateOpenPosition(pos) => {
                    let _ = bot_update_tx.send(MarketUpdate::RelayToFrontend(
                        UpdateFrontend::MarketInfoEdit((
                            asset.name.clone(),
                            EditMarketInfo::OpenPosition(pos),
                        )),
                    ));
                    let _ = engine_update_tx.send(EngineCommand::UpdateExecParams(
                        ExecParam::OpenPosition(pos.map(|p| p.sse())),
                    ));
                }

                MarketCommand::ReceiveTrade(trade_info) => {
                    self.pnl += trade_info.pnl;
                    self.margin += trade_info.pnl;
                    self.trade_history.push(trade_info);
                    let _ = engine_update_tx.send(EngineCommand::UpdateExecParams(
                        ExecParam::Margin(self.margin),
                    ));
                    let _ = bot_update_tx.send(MarketUpdate::RelayToFrontend(
                        UpdateFrontend::MarketInfoEdit((
                            asset.name.clone(),
                            EditMarketInfo::Trade(trade_info),
                        )),
                    ));

                    let _ = bot_update_tx.send(MarketUpdate::MarginUpdate((
                        asset.name.clone(),
                        self.margin,
                    )));
                }

                MarketCommand::UserEvent(event) => {
                    let _ = self.senders.exec_tx.send_async(Event(event)).await;
                }

                MarketCommand::UpdateMargin(marge) => {
                    self.margin = marge;
                    let _ = engine_update_tx.send(EngineCommand::UpdateExecParams(
                        ExecParam::Margin(self.margin),
                    ));
                    let _ = bot_update_tx.send(MarketUpdate::MarginUpdate((
                        asset.name.clone(),
                        self.margin,
                    )));
                }

                MarketCommand::UpdateIndicatorData(data) => {
                    let _ = bot_update_tx.send(MarketUpdate::RelayToFrontend(
                        UpdateFrontend::UpdateIndicatorValues {
                            asset: asset.name.clone(),
                            data,
                        },
                    ));
                }

                MarketCommand::FetchMissedWindow(disc_start) => {
                    info!("FILLING MISSED PRICE WINDOW");
                    let mut map: TimeFrameData = HashMap::default();

                    let end = get_time_now();
                    for tf in &self.active_tfs {
                        if (tf.to_millis()) < (end - disc_start) {
                            let res = candles_snapshot(
                                &self.info_client,
                                asset.name.as_str(),
                                *tf,
                                disc_start,
                                end,
                            )
                            .await?;
                            map.insert(*tf, res);
                        }
                    }
                    if !map.is_empty() {
                        let _ = engine_update_tx.send(EngineCommand::UpdatePriceBulk(map));
                    }
                    disconnection_flag.store(false, Ordering::Relaxed);
                }

                MarketCommand::Pause => {
                    let _ = self
                        .senders
                        .exec_tx
                        .send_async(Control(ExecControl::Pause))
                        .await;
                }

                MarketCommand::Resume => {
                    let _ = self
                        .senders
                        .exec_tx
                        .send_async(Control(ExecControl::Resume))
                        .await;
                }

                MarketCommand::Close => {
                    info!("\nClosing {} Market...\n", asset.name);
                    let _ = engine_update_tx.send(EngineCommand::Stop);
                    //shutdown Executor
                    info!("\nShutting down executor\n");
                    match self.senders.exec_tx.send(Control(ExecControl::Kill)) {
                        Ok(_) => {
                            if let Some(cmd) = self.receivers.market_rv.recv().await {
                                match cmd {
                                    MarketCommand::ReceiveTrade(trade_info) => {
                                        info!(
                                            "\nReceived final trade before shutdown: {:?}\n",
                                            trade_info
                                        );
                                        self.pnl += trade_info.pnl;
                                        self.margin += trade_info.pnl;
                                        self.trade_history.push(trade_info);
                                        let _ = bot_update_tx.send(MarketUpdate::RelayToFrontend(
                                            UpdateFrontend::MarketInfoEdit((
                                                asset.name.clone(),
                                                EditMarketInfo::Trade(trade_info),
                                            )),
                                        ));
                                        break;
                                    }

                                    _ => break,
                                }
                            }
                        }

                        _ => {
                            log::warn!("Cancel message not sent");
                        }
                    }
                    break;
                }
            };
        }

        let _ = engine_handle.await;
        let _ = executor_handle.await;
        let _ = candle_stream_handle.await;
        info!(
            "No. of trade : {}\nPNL: {}",
            &self.trade_history.len(),
            &self.pnl
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MarketCommand {
    UpdateLeverage(usize),      //UI
    UpdateStrategy(Strategy),   //UI
    EditIndicators(Vec<Entry>), //UI
    ReceiveTrade(TradeInfo),    //Exec
    UpdateOpenPosition(Option<OpenPositionLocal>),
    UserEvent(ExecEvent),                    //Bot
    UpdateMargin(f64),                       //UI or Exec
    UpdateIndicatorData(Vec<IndicatorData>), //Engine
    Resume,                                  //UI/Bot
    Pause,                                   //UI/Bot
    Close,                                   //UI/Bot
    FetchMissedWindow(u64),
}

struct MarketSenders {
    bot_tx: UnboundedSender<MarketUpdate>,
    engine_tx: UnboundedSender<EngineCommand>,
    exec_tx: FlumeSender<ExecCommand>,
    market_tx: Sender<MarketCommand>,
}

struct MarketReceivers {
    pub price_rv: UnboundedReceiver<Message>,
    pub market_rv: Receiver<MarketCommand>,
}

#[derive(Debug, Clone)]
pub enum MarketUpdate {
    InitMarket(MarketInfo),
    MarginUpdate(AssetMargin),
    RelayToFrontend(UpdateFrontend),
}

pub type AssetPrice = (String, f64);
