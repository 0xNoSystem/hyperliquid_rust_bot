#![allow(unused_variables)]
use log::warn;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use rhai::Engine;

use alloy::signers::local::PrivateKeySigner;
use hyperliquid_rust_sdk::{AssetMeta, BaseUrl, Error, ExchangeClient, ExchangeResponseStatus};

use crate::backend::scripting::CompiledStrategy;
use crate::broadcast::{CacheCmdIn, CandleCount, CandleSnapshotRequest, PriceData};
use crate::signal::{
    EditType, EngineCommand, EngineView, Entry, ExecParam, ExecParams, IndexId, SignalEngine,
    TimeFrameData,
};
use crate::{AssetMargin, EditMarketInfo, IndicatorData, MarketStream, UpdateFrontend};
use crate::{ExecCommand, ExecControl, ExecEvent, Executor};
use crate::{MarketInfo, Wallet};
use crate::{OpenPositionLocal, TimeFrame, TradeHistory, TradeInfo};

use tokio::sync::mpsc::{Receiver, Sender, UnboundedSender, channel, unbounded_channel};
use tokio::sync::{broadcast, oneshot};
use tokio::task::JoinHandle;

use flume::{Sender as FlumeSender, bounded};

pub struct Market {
    exchange_client: ExchangeClient,
    cache_tx: Sender<CacheCmdIn>,
    pub pnl: f64,
    pub lev: usize,
    strategy: (String, Vec<IndexId>),
    pub asset: AssetMeta,
    signal_engine: SignalEngine,
    executor: Executor,
    receivers: MarketReceivers,
    senders: MarketSenders,
    pub margin: f64,
}

/// Filter out indicator removals that conflict with the strategy's required indicators.
fn filter_edits(required: &[IndexId], edits: &mut Vec<Entry>) -> Vec<IndexId> {
    let mut blocked = Vec::new();
    edits.retain(|e| {
        if e.edit == EditType::Remove && required.contains(&e.id) {
            blocked.push(e.id.clone());
            false
        } else {
            true
        }
    });
    blocked
}

impl Market {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        wallet: Arc<Wallet>,
        bot_tx: UnboundedSender<MarketUpdate>,
        cache_tx: Sender<CacheCmdIn>,
        px_receiver: broadcast::Receiver<PriceData>,
        asset: AssetMeta,
        margin: f64,
        lev: usize,
        rhai_engine: Arc<Engine>,
        compiled: CompiledStrategy,
        strat_indicators: Vec<IndexId>,
        strategy_name: String,
        config: Option<Vec<IndexId>>,
    ) -> Result<(Self, Sender<MarketCommand>), Error> {
        let exchange_client =
            ExchangeClient::new(None, wallet.wallet.clone(), Some(wallet.url), None, None).await?;

        //setup channels
        let (market_tx, market_rv) = channel::<MarketCommand>(7);
        let (exec_tx, exec_rv) = bounded::<ExecCommand>(3);
        let (engine_tx, engine_rv) = unbounded_channel::<EngineCommand>();

        let senders = MarketSenders {
            bot_tx,
            engine_tx,
            exec_tx: exec_tx.clone(),
        };

        let receivers = MarketReceivers {
            price_rv: px_receiver,
            market_rv,
        };

        let lev = lev.min(asset.max_leverage);
        let exec_params = ExecParams::new(margin, lev);

        Ok((
            Market {
                exchange_client,
                cache_tx,
                margin,
                pnl: 0_f64,
                lev,
                strategy: (strategy_name, strat_indicators.clone()),
                asset: asset.clone(),
                signal_engine: SignalEngine::new(
                    config,
                    rhai_engine,
                    compiled,
                    strat_indicators,
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
            },
            market_tx,
        ))
    }

    async fn init(&mut self) -> Result<Option<f64>, Error> {
        //check if lev > max_lev
        let lev = self.lev.min(self.asset.max_leverage);
        Self::update_lev(&self.exchange_client, self.asset.name.as_str(), lev).await?;
        self.lev = lev;

        let engine_tx = self.senders.engine_tx.clone();
        let _ = engine_tx.send(EngineCommand::UpdateExecParams(ExecParam::Lev(self.lev)));

        let last_price = self.load_engine(5000).await?;
        Ok(last_price)
    }

    async fn update_lev(client: &ExchangeClient, asset: &str, lev: usize) -> Result<usize, Error> {
        let response = client
            .update_leverage(lev as u32, asset, false, None)
            .await?;

        match response {
            ExchangeResponseStatus::Ok(_) => Ok(lev),
            ExchangeResponseStatus::Err(e) => Err(Error::Custom(e)),
        }
    }

    async fn load_engine(&mut self, candle_count: CandleCount) -> Result<Option<f64>, Error> {
        let active = self.signal_engine.get_active_indicators();
        let requested_tfs: Vec<TimeFrame> = active
            .iter()
            .map(|id| id.2)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        let request: HashMap<TimeFrame, CandleCount> =
            requested_tfs.iter().map(|tf| (*tf, candle_count)).collect();

        let (reply_tx, reply_rx) = oneshot::channel();
        self.cache_tx
            .send(CacheCmdIn::Snapshot(CandleSnapshotRequest {
                asset: Arc::from(self.asset.name.as_str()),
                request,
                reply: reply_tx,
            }))
            .await
            .map_err(|_| Error::Custom("CandleCache channel closed".into()))?;

        let tf_data = reply_rx
            .await
            .map_err(|_| Error::Custom("CandleCache reply dropped".into()))??;

        let missing: Vec<_> = requested_tfs
            .iter()
            .filter(|tf| !tf_data.contains_key(tf))
            .collect();

        if !missing.is_empty() {
            return Err(Error::Custom(format!(
                "Failed to load candle data for timeframe(s): {}",
                missing
                    .iter()
                    .map(|tf| format!("{tf:?}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )));
        }

        let mut last_price: Option<f64> = None;
        for (tf, prices) in &tf_data {
            last_price = prices.last().map(|p| p.close);
            self.signal_engine
                .load(&Arc::from(self.asset.name.as_str()), *tf, prices.clone())
                .await;
        }

        Ok(last_price)
    }
}

impl Market {
    pub async fn start(mut self) -> Result<(), Error> {
        use ExecCommand::*;
        let last_price = self.init().await?;

        let info = MarketInfo {
            asset: self.asset.name.clone(),
            lev: self.lev,
            price: last_price.unwrap_or(0.0),
            strategy_name: self.strategy.0.clone(),
            margin: self.margin,
            pnl: 0.0,
            is_paused: false,
            indicators: self.signal_engine.get_indicators_data(),
            position: None,
            engine_state: EngineView::Idle,
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
        let mut px_receiver = self.receivers.price_rv;

        let candle_stream_handle: JoinHandle<Result<(), Error>> = tokio::spawn(async move {
            loop {
                match px_receiver.recv().await {
                    Ok(data) => {
                        let last_price = match &data {
                            PriceData::Single(p) => Some(p.close),
                            PriceData::Bulk(ps) => ps.last().map(|p| p.close),
                        };
                        if let Some(px) = last_price {
                            let _ = bot_price_update.send(MarketUpdate::RelayToFrontend(
                                UpdateFrontend::MarketStream(MarketStream::Price {
                                    asset: Arc::clone(&asset_name),
                                    price: px,
                                }),
                            ));
                        }
                        let _ = engine_price_tx
                            .send(EngineCommand::UpdatePrice((Arc::clone(&asset_name), data)));
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!("{} price receiver lagged by {} messages", &asset_name, n);
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        warn!("{} broadcast channel closed", &asset_name);
                        let _ = bot_price_update
                            .send(MarketUpdate::FeedDied((*asset_name).to_string()));
                        break;
                    }
                }
            }
            Ok(())
        });
        //listen to changes and trade results
        let engine_update_tx = self.senders.engine_tx.clone();
        let bot_update_tx = self.senders.bot_tx;
        let asset = self.asset.clone();

        while let Some(cmd) = self.receivers.market_rv.recv().await {
            match cmd {
                MarketCommand::UpdateLeverage(lev) => {
                    let lev = lev.min(asset.max_leverage);
                    if lev == self.lev {
                        continue;
                    }
                    let upd =
                        Self::update_lev(&self.exchange_client, asset.name.as_str(), lev).await;
                    match upd {
                        Ok(lev) => {
                            self.lev = lev;
                            let _ = engine_update_tx
                                .send(EngineCommand::UpdateExecParams(ExecParam::Lev(lev)));

                            let _ = bot_update_tx.send(MarketUpdate::MarketInfoUpdate((
                                asset.name.clone(),
                                EditMarketInfo::Lev(lev),
                            )));
                        }
                        Err(e) => {
                            let _ = bot_update_tx.send(MarketUpdate::RelayToFrontend(
                                UpdateFrontend::UserError(e.to_string()),
                            ));
                        }
                    }
                }

                MarketCommand::UpdateStrategy(compiled, strat_indicators, name) => {
                    let new_tfs: HashMap<TimeFrame, CandleCount> = strat_indicators
                        .iter()
                        .map(|(_, _, tf)| (*tf, 5000))
                        .collect();

                    let mut map: TimeFrameData = HashMap::default();
                    let mut failed = false;
                    if !new_tfs.is_empty() {
                        let (reply_tx, reply_rx) = oneshot::channel();
                        let req = CandleSnapshotRequest {
                            asset: Arc::from(asset.name.as_str()),
                            request: new_tfs.clone(),
                            reply: reply_tx,
                        };
                        if self.cache_tx.send(CacheCmdIn::Snapshot(req)).await.is_ok() {
                            match reply_rx.await {
                                Ok(Ok(data)) => {
                                    let failed_tfs: Vec<_> = new_tfs
                                        .keys()
                                        .filter(|tf| data.get(tf).is_none_or(|v| v.is_empty()))
                                        .copied()
                                        .collect();

                                    if !failed_tfs.is_empty() {
                                        failed = true;
                                        let _ = bot_update_tx.send(MarketUpdate::RelayToFrontend(
                                            UpdateFrontend::UserError(format!(
                                                "Strategy '{}' requires candle data for: {}\nFailed to load — strategy not applied.",
                                                name,
                                                failed_tfs.iter().map(|tf| format!("{tf:?}")).collect::<Vec<_>>().join(", ")
                                            )),
                                        ));
                                    } else {
                                        map = data;
                                    }
                                }
                                Ok(Err(e)) => {
                                    failed = true;
                                    let _ = bot_update_tx.send(MarketUpdate::RelayToFrontend(
                                        UpdateFrontend::UserError(format!(
                                            "Failed to load candle data for strategy '{}': {}",
                                            name, e
                                        )),
                                    ));
                                }
                                Err(_) => {
                                    failed = true;
                                }
                            }
                        }
                    }

                    if failed {
                        continue;
                    }

                    self.strategy = (name, strat_indicators.clone());

                    let _ = engine_update_tx.send(EngineCommand::UpdateStrategy(
                        compiled,
                        strat_indicators.clone(),
                    ));

                    let price_data = if map.is_empty() { None } else { Some(map) };
                    let indicators: Vec<Entry> = strat_indicators
                        .into_iter()
                        .map(|id| Entry {
                            id,
                            edit: EditType::Add,
                        })
                        .collect();
                    let _ = engine_update_tx.send(EngineCommand::EditIndicators {
                        indicators,
                        price_data,
                    });

                    //close any ongoing trade
                    let _ = self
                        .senders
                        .exec_tx
                        .send_async(Control(ExecControl::ForceClose))
                        .await;
                }

                MarketCommand::EditIndicators(mut entry_vec) => {
                    let blocked = filter_edits(&self.strategy.1, &mut entry_vec);
                    if !blocked.is_empty() {
                        let _ = bot_update_tx.send(MarketUpdate::RelayToFrontend(
                            UpdateFrontend::UserError(format!(
                                "Current strategy requires the following indicator(s):\n{}",
                                blocked
                                    .iter()
                                    .map(|id| format!("• {:?}", id))
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            )),
                        ));
                    }

                    let new_tfs: HashMap<TimeFrame, CandleCount> = entry_vec
                        .iter()
                        .filter(|e| e.edit == EditType::Add)
                        .map(|e| (e.id.2, 5000))
                        .collect();

                    let mut map: TimeFrameData = HashMap::default();
                    if !new_tfs.is_empty() {
                        let (reply_tx, reply_rx) = oneshot::channel();
                        let req = CandleSnapshotRequest {
                            asset: asset.name.clone().into(),
                            request: new_tfs.clone(),
                            reply: reply_tx,
                        };
                        if self.cache_tx.send(CacheCmdIn::Snapshot(req)).await.is_ok() {
                            match reply_rx.await {
                                Ok(Ok(data)) => {
                                    // Check which requested TFs are missing or empty
                                    let failed_tfs: Vec<_> = new_tfs
                                        .keys()
                                        .filter(|tf| data.get(tf).is_none_or(|v| v.is_empty()))
                                        .copied()
                                        .collect();

                                    if !failed_tfs.is_empty() {
                                        // Remove indicators that depend on failed TFs
                                        entry_vec.retain(|e| !failed_tfs.contains(&e.id.2));
                                        let _ = bot_update_tx.send(MarketUpdate::RelayToFrontend(
                                            UpdateFrontend::UserError(format!(
                                                "Failed to load candle data for: {}\nIndicators on these timeframes were skipped.",
                                                failed_tfs.iter().map(|tf| format!("{tf:?}")).collect::<Vec<_>>().join(", ")
                                            )),
                                        ));
                                    }

                                    // Only keep TFs that have data
                                    map = data.into_iter().filter(|(_, v)| !v.is_empty()).collect();
                                }
                                Ok(Err(e)) => {
                                    let _ = bot_update_tx.send(MarketUpdate::RelayToFrontend(
                                        UpdateFrontend::UserError(format!(
                                            "Failed to load candle data: {}\nREMOVE CONCERNED INDICATORS AND TRY AGAIN", e
                                        )),
                                    ));
                                }
                                Err(_) => {}
                            }
                        }
                    }

                    let price_data = if map.is_empty() { None } else { Some(map) };
                    let _ = engine_update_tx.send(EngineCommand::EditIndicators {
                        indicators: entry_vec,
                        price_data,
                    });
                }

                MarketCommand::UpdateOpenPosition(pos) => {
                    let _ = bot_update_tx.send(MarketUpdate::MarketInfoUpdate((
                        asset.name.clone(),
                        EditMarketInfo::OpenPosition(pos),
                    )));
                    let _ = engine_update_tx.send(EngineCommand::UpdateExecParams(
                        ExecParam::OpenPosition(pos.map(|p| p.sse())),
                    ));
                }

                MarketCommand::ReceiveTrade(trade_info) => {
                    let _ = bot_update_tx.send(MarketUpdate::MarketInfoUpdate((
                        asset.name.clone(),
                        EditMarketInfo::Trade(trade_info),
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
                        UpdateFrontend::MarketStream(MarketStream::Indicators {
                            asset: Arc::from(asset.name.as_str()),
                            data,
                        }),
                    ));
                }

                MarketCommand::EngineStateChange(new_state) => {
                    let _ = bot_update_tx.send(MarketUpdate::MarketInfoUpdate((
                        asset.name.clone(),
                        EditMarketInfo::EngineState(new_state),
                    )));
                }

                MarketCommand::ManualTradeDetected => {
                    let _ = self.senders.engine_tx.send(EngineCommand::ExecPause);
                    let _ = bot_update_tx.send(MarketUpdate::RelayToFrontend(
                        UpdateFrontend::UserError(format!(
                            "Manual trade detected on {}. Market paused — resume when ready.",
                            asset.name
                        )),
                    ));
                    let _ = bot_update_tx.send(MarketUpdate::MarketInfoUpdate((
                        asset.name.clone(),
                        EditMarketInfo::Paused(true),
                    )));
                }

                MarketCommand::AuthError(msg) => {
                    log::warn!("[market:{}] auth error from executor: {msg}", asset.name);
                    let _ = bot_update_tx.send(MarketUpdate::AuthFailed(msg));
                }

                MarketCommand::ReloadWallet(signer) => {
                    match ExchangeClient::new(
                        None,
                        signer.clone(),
                        Some(BaseUrl::Mainnet),
                        None,
                        None,
                    )
                    .await
                    {
                        Ok(new_client) => {
                            self.exchange_client = new_client;
                            let exec_client = Arc::new(
                                ExchangeClient::new(
                                    None,
                                    signer,
                                    Some(BaseUrl::Mainnet),
                                    None,
                                    None,
                                )
                                .await
                                .expect("ExchangeClient creation failed after first succeeded"),
                            );
                            let _ = self
                                .senders
                                .exec_tx
                                .send_async(ExecCommand::ReloadWallet(exec_client))
                                .await;
                            log::info!("[market:{}] hot-reloaded wallet", asset.name);
                        }
                        Err(e) => {
                            log::error!("[market:{}] failed to reload wallet: {e}", asset.name);
                        }
                    }
                }

                MarketCommand::Pause => {
                    let _ = self
                        .senders
                        .exec_tx
                        .send_async(Control(ExecControl::Pause))
                        .await;
                    let _ = self.senders.engine_tx.send(EngineCommand::ExecPause);
                }

                MarketCommand::Resume => {
                    let _ = self
                        .senders
                        .exec_tx
                        .send_async(Control(ExecControl::Resume))
                        .await;
                    let _ = self.senders.engine_tx.send(EngineCommand::ExecResume);
                }

                MarketCommand::ForceClosePosition => {
                    let _ = self
                        .senders
                        .exec_tx
                        .send_async(Control(ExecControl::ForceClose))
                        .await;
                }

                MarketCommand::Close => {
                    let _ = engine_update_tx.send(EngineCommand::Stop);
                    match self.senders.exec_tx.send(Control(ExecControl::Kill)) {
                        Ok(_) => {
                            if let Some(cmd) = self.receivers.market_rv.recv().await {
                                match cmd {
                                    MarketCommand::ReceiveTrade(trade_info) => {
                                        let _ = bot_update_tx.send(MarketUpdate::MarketInfoUpdate(
                                            (asset.name.clone(), EditMarketInfo::Trade(trade_info)),
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
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MarketCommand {
    UpdateLeverage(usize),
    #[serde(skip)]
    UpdateStrategy(CompiledStrategy, Vec<IndexId>, String), // compiled, indicators, name
    EditIndicators(Vec<Entry>),
    ReceiveTrade(TradeInfo),
    UpdateOpenPosition(Option<OpenPositionLocal>),
    UserEvent(ExecEvent),
    UpdateMargin(f64),
    UpdateIndicatorData(Vec<IndicatorData>),
    EngineStateChange(EngineView),
    ManualTradeDetected,
    ForceClosePosition,
    #[serde(skip)]
    ReloadWallet(PrivateKeySigner),
    #[serde(skip)]
    AuthError(String),
    Resume,
    Pause,
    Close,
}

struct MarketSenders {
    bot_tx: UnboundedSender<MarketUpdate>,
    engine_tx: UnboundedSender<EngineCommand>,
    exec_tx: FlumeSender<ExecCommand>,
}

struct MarketReceivers {
    pub price_rv: broadcast::Receiver<PriceData>,
    pub market_rv: Receiver<MarketCommand>,
}

#[derive(Debug, Clone)]
pub enum MarketUpdate {
    InitMarket(MarketInfo),
    MarginUpdate(AssetMargin),
    MarketInfoUpdate((String, EditMarketInfo)),
    RelayToFrontend(UpdateFrontend),
    AuthFailed(String),
    FeedDied(String), // asset name — Bot should remove this market
}

pub type AssetPrice = (String, f64);

#[derive(Clone, Debug)]
pub struct MarketState {
    pub asset: String,
    pub lev: usize,
    pub strategy_name: String,
    pub margin: f64,
    pub pnl: f64,
    pub is_paused: bool,
    pub position: Option<OpenPositionLocal>,
    pub engine_state: EngineView,
    pub trades: TradeHistory,
}

impl From<&MarketInfo> for MarketState {
    fn from(info: &MarketInfo) -> Self {
        MarketState {
            asset: info.asset.clone(),
            lev: info.lev,
            strategy_name: info.strategy_name.clone(),
            margin: info.margin,
            pnl: info.pnl,
            is_paused: info.is_paused,
            position: info.position,
            engine_state: info.engine_state,
            trades: TradeHistory::default(),
        }
    }
}
