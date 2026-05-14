use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use alloy::signers::local::PrivateKeySigner;
use hyperliquid_rust_sdk::{AssetMeta, BaseUrl, Error, ExchangeClient, ExchangeResponseStatus};

use crate::backend::scripting::{CompiledStrategy, create_engine};
use crate::bot::SyncMarketFeeds;
use crate::broadcast::{CacheCmdIn, CandleCount, CandleSnapshotRequest, PriceAsset, PriceData};
use crate::helper::exchange_client_with_timeout;
use crate::metrics;
use crate::signal::{
    AssetTimeFrameData, EditType, EngineCommand, EngineView, Entry, ExecParam, ExecParams, IndexId,
    SignalEngine, TimeFrameData,
};
use crate::strategy::replace_self_with_asset;
use crate::{
    AssetMargin, BotEvent, EditMarketInfo, IndicatorData, MarketStream, ScriptLog, UpdateFrontend,
};
use crate::{ExecCommand, ExecControl, ExecEvent, Executor};
use crate::{MarketInfo, Wallet};
use crate::{OpenPositionLocal, TimeFrame, TradeHistory, TradeInfo};

use tokio::sync::mpsc::{Receiver, Sender, channel, error::TrySendError};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::{Duration, timeout};

use flume::{Sender as FlumeSender, bounded};

const ENGINE_COMMAND_CHANNEL_SIZE: usize = 512;
const BOT_FEED_SYNC_TIMEOUT_SECS: u64 = 5;
const CANDLE_CACHE_SEND_TIMEOUT_SECS: u64 = 5;
const CANDLE_SNAPSHOT_REPLY_TIMEOUT_SECS: u64 = 60;
const EXEC_COMMAND_SEND_TIMEOUT_SECS: u64 = 5;
const ENGINE_COMMAND_SEND_TIMEOUT_SECS: u64 = 5;
const MARKET_UPDATE_SEND_TIMEOUT_SECS: u64 = 5;
const MARKET_TASK_JOIN_TIMEOUT_SECS: u64 = 5;

pub struct Market {
    exchange_client: ExchangeClient,
    cache_tx: Sender<CacheCmdIn>,
    pub pnl: f64,
    pub lev: usize,
    strategy: (String, Vec<IndexId>),
    manual_indicators: HashSet<IndexId>,
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

fn collect_snapshot_requests<'a>(
    indicators: impl IntoIterator<Item = &'a IndexId>,
    candle_count: CandleCount,
) -> HashMap<Arc<str>, HashMap<TimeFrame, CandleCount>> {
    let mut requests: HashMap<Arc<str>, HashMap<TimeFrame, CandleCount>> = HashMap::new();
    for (asset, _, tf) in indicators {
        requests
            .entry(Arc::clone(asset))
            .or_default()
            .insert(*tf, candle_count);
    }
    requests
}

fn format_asset_timeframes(items: &[(Arc<str>, TimeFrame)]) -> String {
    items
        .iter()
        .map(|(asset, tf)| format!("{asset} {tf:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn manual_indicators_after_edits(
    manual_indicators: &HashSet<IndexId>,
    entry_vec: &[Entry],
) -> HashSet<IndexId> {
    let mut indicators = manual_indicators.clone();
    for entry in entry_vec {
        match entry.edit {
            EditType::Add => {
                indicators.insert(entry.id.clone());
            }
            EditType::Remove => {
                indicators.remove(&entry.id);
            }
        }
    }
    indicators
}

fn strategy_edit_entries(
    manual_indicators: &HashSet<IndexId>,
    current_strategy: &[IndexId],
    next_strategy: &[IndexId],
) -> Vec<Entry> {
    let current_strategy: HashSet<IndexId> = current_strategy.iter().cloned().collect();
    let next_strategy: HashSet<IndexId> = next_strategy.iter().cloned().collect();
    let mut entries = Vec::new();

    let mut removals: Vec<_> = current_strategy
        .difference(&next_strategy)
        .filter(|id| !manual_indicators.contains(*id))
        .cloned()
        .collect();
    removals.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
    entries.extend(removals.into_iter().map(|id| Entry {
        id,
        edit: EditType::Remove,
    }));

    let mut additions: Vec<_> = next_strategy
        .difference(&current_strategy)
        .filter(|id| !manual_indicators.contains(*id))
        .cloned()
        .collect();
    additions.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
    entries.extend(additions.into_iter().map(|id| Entry {
        id,
        edit: EditType::Add,
    }));

    entries
}

fn required_assets_for<'a>(
    base_asset: &str,
    manual_indicators: impl IntoIterator<Item = &'a IndexId>,
    strategy_indicators: impl IntoIterator<Item = &'a IndexId>,
) -> HashSet<Arc<str>> {
    let mut assets = HashSet::from([Arc::<str>::from(base_asset)]);
    for (asset, _, _) in manual_indicators.into_iter().chain(strategy_indicators) {
        assets.insert(Arc::clone(asset));
    }
    assets
}

async fn sync_required_assets_via_bot(
    bot_cmd_tx: &Sender<BotEvent>,
    market: &str,
    required_assets: HashSet<Arc<str>>,
) -> Result<(), Error> {
    let (reply_tx, reply_rx) = oneshot::channel();
    let cmd = BotEvent::SyncMarketFeeds(SyncMarketFeeds {
        market: market.to_string(),
        required_assets: required_assets.into_iter().collect(),
        reply: reply_tx,
    });

    match bot_cmd_tx.try_send(cmd) {
        Ok(()) => {}
        Err(TrySendError::Full(cmd)) => {
            timeout(
                Duration::from_secs(BOT_FEED_SYNC_TIMEOUT_SECS),
                bot_cmd_tx.send(cmd),
            )
            .await
            .map_err(|_| Error::Custom("timed out queuing bot feed sync".into()))?
            .map_err(|_| Error::Custom("bot channel closed".into()))?;
        }
        Err(TrySendError::Closed(_)) => return Err(Error::Custom("bot channel closed".into())),
    }

    timeout(Duration::from_secs(BOT_FEED_SYNC_TIMEOUT_SECS), reply_rx)
        .await
        .map_err(|_| Error::Custom("timed out waiting for bot feed sync".into()))?
        .map_err(|_| Error::Custom("bot feed sync reply dropped".into()))?
}

async fn send_engine_command(
    tx: &Sender<EngineCommand>,
    asset: &str,
    label: &'static str,
    cmd: EngineCommand,
) -> bool {
    match tx.try_send(cmd) {
        Ok(()) => true,
        Err(TrySendError::Full(cmd)) => match timeout(
            Duration::from_secs(ENGINE_COMMAND_SEND_TIMEOUT_SECS),
            tx.send(cmd),
        )
        .await
        {
            Ok(Ok(())) => true,
            Ok(Err(_)) => {
                log::warn!("[market:{asset}] signal engine channel closed while sending {label}");
                false
            }
            Err(_) => {
                log::warn!("[market:{asset}] timed out sending {label} to signal engine");
                false
            }
        },
        Err(TrySendError::Closed(_)) => {
            log::warn!("[market:{asset}] signal engine channel closed while sending {label}");
            false
        }
    }
}

async fn send_market_update(
    tx: &Sender<MarketUpdate>,
    asset: &str,
    label: &'static str,
    update: MarketUpdate,
) -> bool {
    match tx.try_send(update) {
        Ok(()) => true,
        Err(TrySendError::Full(update)) => match timeout(
            Duration::from_secs(MARKET_UPDATE_SEND_TIMEOUT_SECS),
            tx.send(update),
        )
        .await
        {
            Ok(Ok(())) => true,
            Ok(Err(_)) => {
                log::warn!("[market:{asset}] bot update channel closed while sending {label}");
                false
            }
            Err(_) => {
                log::warn!("[market:{asset}] timed out sending {label} to bot update queue");
                false
            }
        },
        Err(TrySendError::Closed(_)) => {
            log::warn!("[market:{asset}] bot update channel closed while sending {label}");
            false
        }
    }
}

async fn send_exec_command(
    tx: &FlumeSender<ExecCommand>,
    asset: &str,
    label: &'static str,
    cmd: ExecCommand,
) -> bool {
    match timeout(
        Duration::from_secs(EXEC_COMMAND_SEND_TIMEOUT_SECS),
        tx.send_async(cmd),
    )
    .await
    {
        Ok(Ok(())) => true,
        Ok(Err(_)) => {
            log::warn!("[market:{asset}] executor channel closed while sending {label}");
            false
        }
        Err(_) => {
            log::warn!("[market:{asset}] timed out sending {label} to executor");
            false
        }
    }
}

async fn join_market_task<T>(
    handle: &mut JoinHandle<T>,
    asset: &str,
    label: &'static str,
) -> Option<T> {
    match timeout(
        Duration::from_secs(MARKET_TASK_JOIN_TIMEOUT_SECS),
        &mut *handle,
    )
    .await
    {
        Ok(Ok(output)) => Some(output),
        Ok(Err(err)) => {
            if !err.is_cancelled() {
                log::warn!("[market:{asset}] {label} task failed: {err}");
            }
            None
        }
        Err(_) => {
            log::warn!("[market:{asset}] timed out stopping {label} task; aborting");
            handle.abort();
            match handle.await {
                Ok(output) => Some(output),
                Err(err) => {
                    if !err.is_cancelled() {
                        log::warn!("[market:{asset}] {label} task failed after abort: {err}");
                    }
                    None
                }
            }
        }
    }
}

impl Market {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        wallet: Arc<Wallet>,
        bot_tx: Sender<MarketUpdate>,
        bot_cmd_tx: Sender<BotEvent>,
        cache_tx: Sender<CacheCmdIn>,
        px_receiver: Receiver<PriceAsset>,
        asset: AssetMeta,
        margin: f64,
        lev: usize,
        compiled: CompiledStrategy,
        mut strat_indicators: Vec<IndexId>,
        strategy_name: String,
        config: Option<Vec<IndexId>>,
    ) -> Result<(Self, Sender<MarketCommand>), Error> {
        if lev == 0 {
            return Err(Error::Custom(
                "leverage must be greater than zero".to_string(),
            ));
        }

        let exchange_client =
            exchange_client_with_timeout("market", wallet.wallet.clone(), wallet.url).await?;
        let manual_indicators: HashSet<IndexId> =
            config.clone().unwrap_or_default().into_iter().collect();

        let (market_tx, market_rv) = channel::<MarketCommand>(7);
        let (exec_tx, exec_rv) = bounded::<ExecCommand>(3);
        let (engine_tx, engine_rv) = channel::<EngineCommand>(ENGINE_COMMAND_CHANNEL_SIZE);
        let (log_tx, log_rv) = channel::<String>(30);

        let mut rhai_engine = create_engine();

        let tx = log_tx.clone();
        rhai_engine.on_print(move |text| {
            if tx.try_send(text.to_string()).is_err() {
                metrics::inc_strategy_log_dropped();
            }
        });

        let senders = MarketSenders {
            bot_tx,
            bot_cmd_tx,
            engine_tx,
            exec_tx: exec_tx.clone(),
        };

        let receivers = MarketReceivers {
            price_rv: px_receiver,
            market_rv,
            log_rv,
        };

        let lev = lev.min(asset.max_leverage);
        let exec_params = ExecParams::new(margin, lev);

        replace_self_with_asset(asset.name.as_str(), &mut strat_indicators);

        let rhai_engine = Arc::new(rhai_engine);
        Ok((
            Market {
                exchange_client,
                cache_tx,
                margin,
                pnl: 0_f64,
                lev,
                strategy: (strategy_name, strat_indicators.clone()),
                manual_indicators,
                asset: asset.clone(),
                signal_engine: SignalEngine::new(
                    asset.name.clone().into(),
                    config,
                    rhai_engine,
                    compiled,
                    strat_indicators,
                    engine_rv,
                    Some(market_tx.clone()),
                    log_tx,
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

        if !send_engine_command(
            &self.senders.engine_tx,
            self.asset.name.as_str(),
            "initial leverage",
            EngineCommand::UpdateExecParams(ExecParam::Lev(self.lev)),
        )
        .await
        {
            return Err(Error::Custom("signal engine channel closed".into()));
        }

        sync_required_assets_via_bot(
            &self.senders.bot_cmd_tx,
            self.asset.name.as_str(),
            self.current_required_assets(),
        )
        .await?;
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
        let requests = collect_snapshot_requests(active.iter(), candle_count);
        let tf_data = Self::fetch_snapshot_map(&self.cache_tx, &requests).await?;

        let missing: Vec<_> = requests
            .iter()
            .flat_map(|(asset, request)| {
                request
                    .keys()
                    .filter(|tf| {
                        tf_data
                            .get(&(Arc::clone(asset), **tf))
                            .filter(|prices| !prices.is_empty())
                            .is_none()
                    })
                    .map(|tf| (Arc::clone(asset), *tf))
            })
            .collect();

        if !missing.is_empty() {
            return Err(Error::Custom(format!(
                "Failed to load candle data for indicator(s): {}",
                format_asset_timeframes(&missing)
            )));
        }

        let mut last_price: Option<f64> = None;
        let mut last_price_ts: Option<u64> = None;
        for ((asset, tf), prices) in &tf_data {
            if asset.as_ref() == self.asset.name.as_str()
                && let Some(price) = prices.last()
                && last_price_ts.is_none_or(|ts| price.close_time >= ts)
            {
                last_price_ts = Some(price.close_time);
                last_price = Some(price.close);
            }
            self.signal_engine.load(asset, *tf, prices.clone()).await;
        }

        Ok(last_price)
    }

    async fn fetch_snapshot(
        cache_tx: &Sender<CacheCmdIn>,
        asset: Arc<str>,
        request: HashMap<TimeFrame, CandleCount>,
    ) -> Result<TimeFrameData, Error> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let cmd = CacheCmdIn::Snapshot(CandleSnapshotRequest {
            asset,
            request,
            reply: reply_tx,
        });

        match cache_tx.try_send(cmd) {
            Ok(()) => {}
            Err(TrySendError::Full(cmd)) => {
                timeout(
                    Duration::from_secs(CANDLE_CACHE_SEND_TIMEOUT_SECS),
                    cache_tx.send(cmd),
                )
                .await
                .map_err(|_| Error::Custom("timed out queuing CandleCache snapshot".into()))?
                .map_err(|_| Error::Custom("CandleCache channel closed".into()))?;
            }
            Err(TrySendError::Closed(_)) => {
                return Err(Error::Custom("CandleCache channel closed".into()));
            }
        }

        timeout(
            Duration::from_secs(CANDLE_SNAPSHOT_REPLY_TIMEOUT_SECS),
            reply_rx,
        )
        .await
        .map_err(|_| Error::Custom("timed out waiting for CandleCache snapshot".into()))?
        .map_err(|_| Error::Custom("CandleCache reply dropped".into()))?
    }

    async fn fetch_snapshot_map(
        cache_tx: &Sender<CacheCmdIn>,
        requests: &HashMap<Arc<str>, HashMap<TimeFrame, CandleCount>>,
    ) -> Result<AssetTimeFrameData, Error> {
        let mut snapshot_map = AssetTimeFrameData::default();

        for (asset, request) in requests {
            let tf_data =
                Self::fetch_snapshot(cache_tx, Arc::clone(asset), request.clone()).await?;

            for (tf, prices) in tf_data {
                snapshot_map.insert((Arc::clone(asset), tf), prices);
            }
        }

        Ok(snapshot_map)
    }

    fn current_required_assets(&self) -> HashSet<Arc<str>> {
        Self::required_assets_for(
            self.asset.name.as_str(),
            self.manual_indicators.iter(),
            self.strategy.1.iter(),
        )
    }

    fn required_assets_for<'a>(
        base_asset: &str,
        manual_indicators: impl IntoIterator<Item = &'a IndexId>,
        strategy_indicators: impl IntoIterator<Item = &'a IndexId>,
    ) -> HashSet<Arc<str>> {
        let mut assets = HashSet::from([Arc::<str>::from(base_asset)]);
        for (asset, _, _) in manual_indicators.into_iter().chain(strategy_indicators) {
            assets.insert(Arc::clone(asset));
        }
        assets
    }

    async fn forward_close_update(
        bot_update_tx: &Sender<MarketUpdate>,
        asset: &str,
        cmd: MarketCommand,
    ) {
        match cmd {
            MarketCommand::ReceiveTrade(trade_info) => {
                let _ = send_market_update(
                    bot_update_tx,
                    asset,
                    "close trade update",
                    MarketUpdate::MarketInfoUpdate((
                        asset.to_string(),
                        EditMarketInfo::Trade(trade_info),
                    )),
                )
                .await;
            }
            MarketCommand::UpdateOpenPosition(pos) => {
                let _ = send_market_update(
                    bot_update_tx,
                    asset,
                    "close position update",
                    MarketUpdate::MarketInfoUpdate((
                        asset.to_string(),
                        EditMarketInfo::OpenPosition(pos),
                    )),
                )
                .await;
            }
            MarketCommand::EngineStateChange(new_state) => {
                let _ = send_market_update(
                    bot_update_tx,
                    asset,
                    "close engine-state update",
                    MarketUpdate::MarketInfoUpdate((
                        asset.to_string(),
                        EditMarketInfo::EngineState(new_state),
                    )),
                )
                .await;
            }
            MarketCommand::AuthError(msg) => {
                let _ = send_market_update(
                    bot_update_tx,
                    asset,
                    "auth failure",
                    MarketUpdate::AuthFailed(msg),
                )
                .await;
            }
            _ => {}
        }
    }

    async fn drain_close_updates(
        market_rv: &mut Receiver<MarketCommand>,
        bot_update_tx: &Sender<MarketUpdate>,
        asset: &str,
    ) {
        while let Ok(cmd) = market_rv.try_recv() {
            Self::forward_close_update(bot_update_tx, asset, cmd).await;
        }
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
        let _ = send_market_update(
            &self.senders.bot_tx,
            self.asset.name.as_str(),
            "market init",
            MarketUpdate::InitMarket(info),
        )
        .await;

        let mut signal_engine = self.signal_engine;
        let mut executor = self.executor;

        //Start engine
        let mut engine_handle = tokio::spawn(async move {
            signal_engine.start().await;
        });
        //Start exucutor
        let mut executor_handle = tokio::spawn(async move {
            executor.start().await;
        });
        let mut executor_joined = false;
        //Candle Stream
        let engine_price_tx = self.senders.engine_tx.clone();
        let bot_price_update = self.senders.bot_tx.clone();
        let asset_name: Arc<str> = Arc::from(self.asset.name.clone());
        let mut px_receiver = self.receivers.price_rv;

        let mut candle_stream_handle: JoinHandle<Result<(), Error>> = tokio::spawn(async move {
            let mut engine_backpressure_warned = false;
            let mut frontend_price_backpressure_warned = false;
            while let Some((tick_asset, data)) = px_receiver.recv().await {
                if tick_asset == asset_name {
                    let last_price = match &data {
                        PriceData::Single(p) => Some(p.close),
                        PriceData::Bulk(ps) => ps.last().map(|p| p.close),
                    };
                    if let Some(px) = last_price {
                        let update = MarketUpdate::RelayToFrontend(UpdateFrontend::MarketStream(
                            MarketStream::Price {
                                asset: Arc::clone(&asset_name),
                                price: px,
                            },
                        ));
                        match bot_price_update.try_send(update) {
                            Ok(()) => frontend_price_backpressure_warned = false,
                            Err(TrySendError::Full(_)) => {
                                metrics::inc_market_frontend_price_dropped();
                                if !frontend_price_backpressure_warned {
                                    log::warn!(
                                        "bot update queue full for {}; dropping frontend price updates",
                                        &asset_name
                                    );
                                    frontend_price_backpressure_warned = true;
                                }
                            }
                            Err(TrySendError::Closed(_)) => {
                                log::warn!(
                                    "bot update queue closed for {}; stopping market price bridge",
                                    &asset_name
                                );
                                break;
                            }
                        }
                    }
                }
                match engine_price_tx.try_send(EngineCommand::UpdatePrice((tick_asset, data))) {
                    Ok(()) => engine_backpressure_warned = false,
                    Err(TrySendError::Full(_)) => {
                        metrics::inc_signal_engine_price_dropped();
                        if !engine_backpressure_warned {
                            log::warn!(
                                "signal engine price queue full; dropping live price updates"
                            );
                            engine_backpressure_warned = true;
                        }
                    }
                    Err(TrySendError::Closed(_)) => {
                        log::warn!("signal engine price channel closed; stopping price bridge");
                        break;
                    }
                }
            }
            //let _ = bot_price_update.send(MarketUpdate::FeedDied((*asset_name).to_string()));
            Ok(())
        });

        //Relay Stategy logs to user
        let log_sender = self.senders.bot_tx.clone();
        let mut log_rv = self.receivers.log_rv;
        let asset: Arc<str> = Arc::from(self.asset.name.clone());
        tokio::spawn(async move {
            let mut queue_full = false;
            let mut dropped = 0_u64;
            while let Some(msg) = log_rv.recv().await {
                let update =
                    MarketUpdate::RelayToFrontend(UpdateFrontend::StrategyLog(ScriptLog {
                        asset: asset.clone(),
                        msg,
                    }));
                match log_sender.try_send(update) {
                    Ok(()) => {
                        if queue_full {
                            log::info!(
                                "strategy log queue recovered for {} after dropping {} logs",
                                &asset,
                                dropped
                            );
                            queue_full = false;
                            dropped = 0;
                        }
                    }
                    Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                        metrics::inc_strategy_log_dropped();
                        dropped = dropped.saturating_add(1);
                        if !queue_full {
                            log::warn!("strategy log queue full for {}; dropping logs", &asset);
                            queue_full = true;
                        }
                    }
                    Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => break,
                }
            }
        });
        //listen to changes and trade results
        let engine_update_tx = self.senders.engine_tx.clone();
        let bot_update_tx = self.senders.bot_tx;
        let asset = self.asset.clone();

        while let Some(cmd) = self.receivers.market_rv.recv().await {
            match cmd {
                MarketCommand::UpdateLeverage(lev) => {
                    if lev == 0 {
                        let _ = send_market_update(
                            &bot_update_tx,
                            asset.name.as_str(),
                            "leverage error",
                            MarketUpdate::RelayToFrontend(UpdateFrontend::UserError(
                                "Leverage must be greater than zero".to_string(),
                            )),
                        )
                        .await;
                        continue;
                    }

                    let lev = lev.min(asset.max_leverage);
                    if lev == self.lev {
                        continue;
                    }
                    let upd =
                        Self::update_lev(&self.exchange_client, asset.name.as_str(), lev).await;
                    match upd {
                        Ok(lev) => {
                            self.lev = lev;
                            let _ = send_engine_command(
                                &engine_update_tx,
                                asset.name.as_str(),
                                "leverage update",
                                EngineCommand::UpdateExecParams(ExecParam::Lev(lev)),
                            )
                            .await;

                            let _ = send_market_update(
                                &bot_update_tx,
                                asset.name.as_str(),
                                "leverage update",
                                MarketUpdate::MarketInfoUpdate((
                                    asset.name.clone(),
                                    EditMarketInfo::Lev(lev),
                                )),
                            )
                            .await;
                        }
                        Err(e) => {
                            let _ = send_market_update(
                                &bot_update_tx,
                                asset.name.as_str(),
                                "leverage error",
                                MarketUpdate::RelayToFrontend(UpdateFrontend::UserError(
                                    e.to_string(),
                                )),
                            )
                            .await;
                        }
                    }
                }

                MarketCommand::UpdateStrategy(compiled, mut strat_indicators, name) => {
                    replace_self_with_asset(asset.name.as_str(), &mut strat_indicators);

                    let strategy_entries = strategy_edit_entries(
                        &self.manual_indicators,
                        &self.strategy.1,
                        &strat_indicators,
                    );
                    let requests = collect_snapshot_requests(
                        strategy_entries
                            .iter()
                            .filter(|entry| entry.edit == EditType::Add)
                            .map(|entry| &entry.id),
                        5000,
                    );
                    let mut map = AssetTimeFrameData::default();
                    let mut failed = false;
                    if !requests.is_empty() {
                        match Self::fetch_snapshot_map(&self.cache_tx, &requests).await {
                            Ok(data) => {
                                let failed_keys: Vec<_> = requests
                                    .iter()
                                    .flat_map(|(req_asset, request)| {
                                        request
                                            .keys()
                                            .filter(|tf| {
                                                data.get(&(Arc::clone(req_asset), **tf))
                                                    .filter(|prices| !prices.is_empty())
                                                    .is_none()
                                            })
                                            .map(|tf| (Arc::clone(req_asset), *tf))
                                    })
                                    .collect();

                                if !failed_keys.is_empty() {
                                    failed = true;
                                    let _ = send_market_update(
                                        &bot_update_tx,
                                        asset.name.as_str(),
                                        "strategy candle error",
                                        MarketUpdate::RelayToFrontend(
                                            UpdateFrontend::UserError(format!(
                                                "Strategy '{}' requires candle data for: {}\nFailed to load — strategy not applied.",
                                                name,
                                                format_asset_timeframes(&failed_keys)
                                            )),
                                        ),
                                    )
                                    .await;
                                } else {
                                    map = data;
                                }
                            }
                            Err(e) => {
                                failed = true;
                                let _ = send_market_update(
                                    &bot_update_tx,
                                    asset.name.as_str(),
                                    "strategy candle error",
                                    MarketUpdate::RelayToFrontend(
                                        UpdateFrontend::UserError(format!(
                                            "Failed to load candle data for strategy '{}': {}\nStrategy not applied.",
                                            name, e
                                        )),
                                    ),
                                )
                                .await;
                            }
                        }
                    }

                    if failed {
                        continue;
                    }

                    if let Err(e) = sync_required_assets_via_bot(
                        &self.senders.bot_cmd_tx,
                        asset.name.as_str(),
                        required_assets_for(
                            asset.name.as_str(),
                            self.manual_indicators.iter(),
                            strat_indicators.iter(),
                        ),
                    )
                    .await
                    {
                        let _ = send_market_update(
                            &bot_update_tx,
                            asset.name.as_str(),
                            "strategy feed-sync error",
                            MarketUpdate::RelayToFrontend(UpdateFrontend::UserError(format!(
                                "Failed to sync live feeds for strategy '{}': {}",
                                name, e
                            ))),
                        )
                        .await;
                        continue;
                    }

                    self.strategy = (name, strat_indicators.clone());

                    let _ = send_engine_command(
                        &engine_update_tx,
                        asset.name.as_str(),
                        "strategy update",
                        EngineCommand::UpdateStrategy(compiled, strat_indicators.clone()),
                    )
                    .await;

                    if !strategy_entries.is_empty() || !map.is_empty() {
                        let price_data = if map.is_empty() { None } else { Some(map) };
                        let _ = send_engine_command(
                            &engine_update_tx,
                            asset.name.as_str(),
                            "strategy indicator edit",
                            EngineCommand::EditIndicators {
                                indicators: strategy_entries,
                                price_data,
                            },
                        )
                        .await;
                    }

                    //close any ongoing trade
                    send_exec_command(
                        &self.senders.exec_tx,
                        asset.name.as_str(),
                        "force-close after strategy update",
                        Control(ExecControl::ForceClose),
                    )
                    .await;
                }

                MarketCommand::EditIndicators(mut entry_vec) => {
                    let blocked = filter_edits(&self.strategy.1, &mut entry_vec);
                    if !blocked.is_empty() {
                        let _ = send_market_update(
                            &bot_update_tx,
                            asset.name.as_str(),
                            "indicator edit blocked",
                            MarketUpdate::RelayToFrontend(UpdateFrontend::UserError(format!(
                                "Current strategy requires the following indicator(s):\n{}",
                                blocked
                                    .iter()
                                    .map(|id| format!("• {:?}", id))
                                    .collect::<Vec<_>>()
                                    .join("\n")
                            ))),
                        )
                        .await;
                    }

                    let requests = collect_snapshot_requests(
                        entry_vec
                            .iter()
                            .filter(|e| e.edit == EditType::Add)
                            .map(|e| &e.id),
                        5000,
                    );

                    let mut map = AssetTimeFrameData::default();
                    if !requests.is_empty() {
                        match Self::fetch_snapshot_map(&self.cache_tx, &requests).await {
                            Ok(data) => {
                                let failed_keys: HashSet<_> = requests
                                    .iter()
                                    .flat_map(|(req_asset, request)| {
                                        request
                                            .keys()
                                            .filter(|tf| {
                                                data.get(&(Arc::clone(req_asset), **tf))
                                                    .filter(|prices| !prices.is_empty())
                                                    .is_none()
                                            })
                                            .map(|tf| (Arc::clone(req_asset), *tf))
                                    })
                                    .collect();

                                if !failed_keys.is_empty() {
                                    entry_vec.retain(|e| {
                                        !failed_keys.contains(&(Arc::clone(&e.id.0), e.id.2))
                                    });
                                    let mut failed_list: Vec<_> =
                                        failed_keys.iter().cloned().collect();
                                    failed_list.sort_by(|(asset_a, tf_a), (asset_b, tf_b)| {
                                        asset_a.as_ref().cmp(asset_b.as_ref()).then_with(|| {
                                            format!("{tf_a:?}").cmp(&format!("{tf_b:?}"))
                                        })
                                    });
                                    let _ = send_market_update(
                                        &bot_update_tx,
                                        asset.name.as_str(),
                                        "indicator candle error",
                                        MarketUpdate::RelayToFrontend(
                                            UpdateFrontend::UserError(format!(
                                                "Failed to load candle data for: {}\nIndicators on these asset/timeframes were skipped.",
                                                format_asset_timeframes(&failed_list)
                                            )),
                                        ),
                                    )
                                    .await;
                                }

                                map = data
                                    .into_iter()
                                    .filter(|(_, prices)| !prices.is_empty())
                                    .filter(|((req_asset, tf), _)| {
                                        !failed_keys.contains(&(Arc::clone(req_asset), *tf))
                                    })
                                    .collect();
                            }
                            Err(e) => {
                                let _ = send_market_update(
                                    &bot_update_tx,
                                    asset.name.as_str(),
                                    "indicator candle error",
                                    MarketUpdate::RelayToFrontend(
                                        UpdateFrontend::UserError(format!(
                                            "Failed to load candle data: {}\nIndicator changes were not applied.",
                                            e
                                        )),
                                    ),
                                )
                                .await;
                                continue;
                            }
                        }
                    }

                    let next_manual_indicators =
                        manual_indicators_after_edits(&self.manual_indicators, &entry_vec);
                    if let Err(e) = sync_required_assets_via_bot(
                        &self.senders.bot_cmd_tx,
                        asset.name.as_str(),
                        required_assets_for(
                            asset.name.as_str(),
                            next_manual_indicators.iter(),
                            self.strategy.1.iter(),
                        ),
                    )
                    .await
                    {
                        let _ = send_market_update(
                            &bot_update_tx,
                            asset.name.as_str(),
                            "indicator feed-sync error",
                            MarketUpdate::RelayToFrontend(UpdateFrontend::UserError(format!(
                                "Failed to sync live feeds for indicator update: {}",
                                e
                            ))),
                        )
                        .await;
                        continue;
                    }

                    let price_data = if map.is_empty() { None } else { Some(map) };
                    let _ = send_engine_command(
                        &engine_update_tx,
                        asset.name.as_str(),
                        "indicator edit",
                        EngineCommand::EditIndicators {
                            indicators: entry_vec.clone(),
                            price_data,
                        },
                    )
                    .await;
                    self.manual_indicators = next_manual_indicators;
                }

                MarketCommand::UpdateOpenPosition(pos) => {
                    let _ = send_market_update(
                        &bot_update_tx,
                        asset.name.as_str(),
                        "open-position update",
                        MarketUpdate::MarketInfoUpdate((
                            asset.name.clone(),
                            EditMarketInfo::OpenPosition(pos),
                        )),
                    )
                    .await;
                    let _ = send_engine_command(
                        &engine_update_tx,
                        asset.name.as_str(),
                        "open-position update",
                        EngineCommand::UpdateExecParams(ExecParam::OpenPosition(
                            pos.map(|p| p.sse()),
                        )),
                    )
                    .await;
                }

                MarketCommand::ReceiveTrade(trade_info) => {
                    let _ = send_market_update(
                        &bot_update_tx,
                        asset.name.as_str(),
                        "trade update",
                        MarketUpdate::MarketInfoUpdate((
                            asset.name.clone(),
                            EditMarketInfo::Trade(trade_info),
                        )),
                    )
                    .await;
                }

                MarketCommand::UserEvent(event) => {
                    send_exec_command(
                        &self.senders.exec_tx,
                        asset.name.as_str(),
                        "user event",
                        Event(event),
                    )
                    .await;
                }

                MarketCommand::UpdateMargin(marge) => {
                    self.margin = marge;
                    let _ = send_engine_command(
                        &engine_update_tx,
                        asset.name.as_str(),
                        "margin update",
                        EngineCommand::UpdateExecParams(ExecParam::Margin(self.margin)),
                    )
                    .await;
                    let _ = send_market_update(
                        &bot_update_tx,
                        asset.name.as_str(),
                        "margin update",
                        MarketUpdate::MarginUpdate((asset.name.clone(), self.margin)),
                    )
                    .await;
                }

                MarketCommand::UpdateIndicatorData(data) => {
                    let _ = send_market_update(
                        &bot_update_tx,
                        asset.name.as_str(),
                        "indicator stream update",
                        MarketUpdate::RelayToFrontend(UpdateFrontend::MarketStream(
                            MarketStream::Indicators {
                                asset: Arc::from(asset.name.as_str()),
                                data,
                            },
                        )),
                    )
                    .await;
                }

                MarketCommand::EngineStateChange(new_state) => {
                    let _ = send_market_update(
                        &bot_update_tx,
                        asset.name.as_str(),
                        "engine-state update",
                        MarketUpdate::MarketInfoUpdate((
                            asset.name.clone(),
                            EditMarketInfo::EngineState(new_state),
                        )),
                    )
                    .await;
                }

                MarketCommand::ManualTradeDetected => {
                    let _ = send_engine_command(
                        &self.senders.engine_tx,
                        asset.name.as_str(),
                        "manual-trade pause",
                        EngineCommand::ExecPause,
                    )
                    .await;
                    let _ = send_market_update(
                        &bot_update_tx,
                        asset.name.as_str(),
                        "manual-trade notice",
                        MarketUpdate::RelayToFrontend(UpdateFrontend::UserError(format!(
                            "Manual trade detected on {}. Market paused — resume when ready.",
                            asset.name
                        ))),
                    )
                    .await;
                    let _ = send_market_update(
                        &bot_update_tx,
                        asset.name.as_str(),
                        "manual-trade paused state",
                        MarketUpdate::MarketInfoUpdate((
                            asset.name.clone(),
                            EditMarketInfo::Paused(true),
                        )),
                    )
                    .await;
                }

                MarketCommand::AuthError(msg) => {
                    log::warn!("[market:{}] auth error from executor: {msg}", asset.name);
                    let _ = send_market_update(
                        &bot_update_tx,
                        asset.name.as_str(),
                        "auth failure",
                        MarketUpdate::AuthFailed(msg),
                    )
                    .await;
                }

                MarketCommand::ReloadWallet(signer) => {
                    let market_client = exchange_client_with_timeout(
                        "market wallet reload",
                        signer.clone(),
                        BaseUrl::Mainnet,
                    );
                    let exec_client = exchange_client_with_timeout(
                        "executor wallet reload",
                        signer,
                        BaseUrl::Mainnet,
                    );
                    let (market_client, exec_client) = tokio::join!(market_client, exec_client);

                    match (market_client, exec_client) {
                        (Ok(new_client), Ok(exec_client)) => {
                            self.exchange_client = new_client;
                            send_exec_command(
                                &self.senders.exec_tx,
                                asset.name.as_str(),
                                "wallet reload",
                                ExecCommand::ReloadWallet(Arc::new(exec_client)),
                            )
                            .await;
                            log::info!("[market:{}] hot-reloaded wallet", asset.name);
                        }
                        (Err(e), _) | (_, Err(e)) => {
                            log::error!(
                                "[market:{}] failed to reload exchange client: {e}",
                                asset.name
                            );
                        }
                    }
                }

                MarketCommand::Pause => {
                    send_exec_command(
                        &self.senders.exec_tx,
                        asset.name.as_str(),
                        "pause",
                        Control(ExecControl::Pause),
                    )
                    .await;
                    let _ = send_engine_command(
                        &self.senders.engine_tx,
                        asset.name.as_str(),
                        "pause",
                        EngineCommand::ExecPause,
                    )
                    .await;
                }

                MarketCommand::Resume => {
                    send_exec_command(
                        &self.senders.exec_tx,
                        asset.name.as_str(),
                        "resume",
                        Control(ExecControl::Resume),
                    )
                    .await;
                    let _ = send_engine_command(
                        &self.senders.engine_tx,
                        asset.name.as_str(),
                        "resume",
                        EngineCommand::ExecResume,
                    )
                    .await;
                }

                MarketCommand::ForceClosePosition => {
                    send_exec_command(
                        &self.senders.exec_tx,
                        asset.name.as_str(),
                        "force close",
                        Control(ExecControl::ForceClose),
                    )
                    .await;
                }

                MarketCommand::Close => {
                    let _ = send_engine_command(
                        &engine_update_tx,
                        asset.name.as_str(),
                        "stop",
                        EngineCommand::Stop,
                    )
                    .await;
                    if send_exec_command(
                        &self.senders.exec_tx,
                        asset.name.as_str(),
                        "kill",
                        Control(ExecControl::Kill),
                    )
                    .await
                    {
                        loop {
                            tokio::select! {
                                maybe_cmd = self.receivers.market_rv.recv() => {
                                    let Some(cmd) = maybe_cmd else {
                                        break;
                                    };

                                    Self::forward_close_update(
                                        &bot_update_tx,
                                        asset.name.as_str(),
                                        cmd,
                                    )
                                    .await;
                                }
                                result = &mut executor_handle => {
                                    if let Err(err) = result {
                                        log::warn!(
                                            "[market:{}] executor task failed during close: {err}",
                                            asset.name
                                        );
                                    }
                                    executor_joined = true;
                                    Self::drain_close_updates(
                                        &mut self.receivers.market_rv,
                                        &bot_update_tx,
                                        asset.name.as_str(),
                                    )
                                    .await;
                                    break;
                                }
                            }
                        }
                    }
                    break;
                }
            };
        }

        candle_stream_handle.abort();
        let _ = join_market_task(
            &mut candle_stream_handle,
            asset.name.as_str(),
            "price bridge",
        )
        .await;

        let _ = send_engine_command(
            &engine_update_tx,
            asset.name.as_str(),
            "shutdown",
            EngineCommand::Stop,
        )
        .await;
        if !executor_joined {
            let _ = send_exec_command(
                &self.senders.exec_tx,
                asset.name.as_str(),
                "shutdown",
                Control(ExecControl::Kill),
            )
            .await;
        }

        let _ = join_market_task(&mut engine_handle, asset.name.as_str(), "signal engine").await;
        if !executor_joined {
            let _ = join_market_task(&mut executor_handle, asset.name.as_str(), "executor").await;
        }
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
    bot_tx: Sender<MarketUpdate>,
    bot_cmd_tx: Sender<BotEvent>,
    engine_tx: Sender<EngineCommand>,
    exec_tx: FlumeSender<ExecCommand>,
}

struct MarketReceivers {
    pub price_rv: Receiver<PriceAsset>,
    pub market_rv: Receiver<MarketCommand>,
    pub log_rv: Receiver<String>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sync_required_assets_via_bot_sends_request_and_returns_reply() {
        let (tx, mut rx) = channel(1);
        let market = "BTC".to_string();
        let asset = Arc::<str>::from("ETH");

        let worker = tokio::spawn(async move {
            let Some(BotEvent::SyncMarketFeeds(payload)) = rx.recv().await else {
                panic!("expected SyncMarketFeeds command");
            };

            assert_eq!(payload.market, market);
            assert_eq!(payload.required_assets, vec![asset]);
            let _ = payload.reply.send(Ok(()));
        });

        let mut required_assets = HashSet::new();
        required_assets.insert(Arc::<str>::from("ETH"));

        sync_required_assets_via_bot(&tx, "BTC", required_assets)
            .await
            .expect("feed sync should complete");

        worker.await.expect("worker should finish");
    }

    #[tokio::test]
    async fn sync_required_assets_via_bot_errors_when_bot_channel_closed() {
        let (tx, rx) = channel(1);
        drop(rx);

        let err = sync_required_assets_via_bot(&tx, "BTC", HashSet::new())
            .await
            .expect_err("closed bot channel should fail");

        assert!(err.to_string().contains("bot channel closed"));
    }

    #[tokio::test]
    async fn fetch_snapshot_sends_request_and_returns_reply() {
        let (tx, mut rx) = channel(1);
        let asset = Arc::<str>::from("BTC");

        let worker = tokio::spawn(async move {
            let Some(CacheCmdIn::Snapshot(snapshot)) = rx.recv().await else {
                panic!("expected CandleCache snapshot command");
            };

            assert_eq!(snapshot.asset, asset);
            assert_eq!(snapshot.request.get(&TimeFrame::Min1), Some(&10));
            let _ = snapshot.reply.send(Ok(TimeFrameData::default()));
        });

        let mut request = HashMap::new();
        request.insert(TimeFrame::Min1, 10);

        let data = Market::fetch_snapshot(&tx, Arc::<str>::from("BTC"), request)
            .await
            .expect("snapshot request should complete");

        assert!(data.is_empty());
        worker.await.expect("worker should finish");
    }

    #[tokio::test]
    async fn fetch_snapshot_errors_when_cache_channel_closed() {
        let (tx, rx) = channel(1);
        drop(rx);

        let err = Market::fetch_snapshot(&tx, Arc::<str>::from("BTC"), HashMap::new())
            .await
            .expect_err("closed cache channel should fail");

        assert!(err.to_string().contains("CandleCache channel closed"));
    }
}
