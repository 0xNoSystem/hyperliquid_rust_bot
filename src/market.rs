#![allow(unused_variables)]
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use alloy::signers::local::PrivateKeySigner;
use hyperliquid_rust_sdk::{AssetMeta, BaseUrl, Error, ExchangeClient, ExchangeResponseStatus};

use crate::backend::scripting::{CompiledStrategy, create_engine};
use crate::bot::SyncMarketFeeds;
use crate::broadcast::{CacheCmdIn, CandleCount, CandleSnapshotRequest, PriceAsset, PriceData};
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

use tokio::sync::mpsc::{
    Receiver, Sender, UnboundedReceiver, UnboundedSender, channel, unbounded_channel,
};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use flume::{Sender as FlumeSender, bounded};

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
    bot_cmd_tx
        .send(BotEvent::SyncMarketFeeds(SyncMarketFeeds {
            market: market.to_string(),
            required_assets: required_assets.into_iter().collect(),
            reply: reply_tx,
        }))
        .await
        .map_err(|_| Error::Custom("bot channel closed".into()))?;

    reply_rx
        .await
        .map_err(|_| Error::Custom("bot feed sync reply dropped".into()))?
}

impl Market {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        wallet: Arc<Wallet>,
        bot_tx: UnboundedSender<MarketUpdate>,
        bot_cmd_tx: Sender<BotEvent>,
        cache_tx: Sender<CacheCmdIn>,
        px_receiver: UnboundedReceiver<PriceAsset>,
        asset: AssetMeta,
        margin: f64,
        lev: usize,
        compiled: CompiledStrategy,
        mut strat_indicators: Vec<IndexId>,
        strategy_name: String,
        config: Option<Vec<IndexId>>,
    ) -> Result<(Self, Sender<MarketCommand>), Error> {
        let exchange_client =
            ExchangeClient::new(None, wallet.wallet.clone(), Some(wallet.url), None, None).await?;
        let manual_indicators: HashSet<IndexId> =
            config.clone().unwrap_or_default().into_iter().collect();

        let (market_tx, market_rv) = channel::<MarketCommand>(7);
        let (exec_tx, exec_rv) = bounded::<ExecCommand>(3);
        let (engine_tx, engine_rv) = unbounded_channel::<EngineCommand>();
        let (log_tx, log_rv) = channel::<String>(30);

        let mut rhai_engine = create_engine();

        let tx = log_tx.clone();
        rhai_engine.on_print(move |text| {
            let _ = tx.try_send(text.to_string());
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

        let engine_tx = self.senders.engine_tx.clone();
        let _ = engine_tx.send(EngineCommand::UpdateExecParams(ExecParam::Lev(self.lev)));

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
        cache_tx
            .send(CacheCmdIn::Snapshot(CandleSnapshotRequest {
                asset,
                request,
                reply: reply_tx,
            }))
            .await
            .map_err(|_| Error::Custom("CandleCache channel closed".into()))?;

        reply_rx
            .await
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
            while let Some((tick_asset, data)) = px_receiver.recv().await {
                if tick_asset == asset_name {
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
                }
                let _ = engine_price_tx.send(EngineCommand::UpdatePrice((tick_asset, data)));
            }
            //let _ = bot_price_update.send(MarketUpdate::FeedDied((*asset_name).to_string()));
            Ok(())
        });

        //Relay Stategy logs to user
        let log_sender = self.senders.bot_tx.clone();
        let mut log_rv = self.receivers.log_rv;
        let asset: Arc<str> = Arc::from(self.asset.name.clone());
        tokio::spawn(async move {
            while let Some(msg) = log_rv.recv().await {
                let _ = log_sender.send(MarketUpdate::RelayToFrontend(
                    UpdateFrontend::StrategyLog(ScriptLog {
                        asset: asset.clone(),
                        msg,
                    }),
                ));
            }
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
                                    let _ = bot_update_tx.send(MarketUpdate::RelayToFrontend(
                                        UpdateFrontend::UserError(format!(
                                            "Strategy '{}' requires candle data for: {}\nFailed to load — strategy not applied.",
                                            name,
                                            format_asset_timeframes(&failed_keys)
                                        )),
                                    ));
                                } else {
                                    map = data;
                                }
                            }
                            Err(e) => {
                                failed = true;
                                let _ = bot_update_tx.send(MarketUpdate::RelayToFrontend(
                                    UpdateFrontend::UserError(format!(
                                        "Failed to load candle data for strategy '{}': {}\nStrategy not applied.",
                                        name, e
                                    )),
                                ));
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
                        let _ = bot_update_tx.send(MarketUpdate::RelayToFrontend(
                            UpdateFrontend::UserError(format!(
                                "Failed to sync live feeds for strategy '{}': {}",
                                name, e
                            )),
                        ));
                        continue;
                    }

                    self.strategy = (name, strat_indicators.clone());

                    let _ = engine_update_tx.send(EngineCommand::UpdateStrategy(
                        compiled,
                        strat_indicators.clone(),
                    ));

                    if !strategy_entries.is_empty() || !map.is_empty() {
                        let price_data = if map.is_empty() { None } else { Some(map) };
                        let _ = engine_update_tx.send(EngineCommand::EditIndicators {
                            indicators: strategy_entries,
                            price_data,
                        });
                    }

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
                                    let _ = bot_update_tx.send(MarketUpdate::RelayToFrontend(
                                        UpdateFrontend::UserError(format!(
                                            "Failed to load candle data for: {}\nIndicators on these asset/timeframes were skipped.",
                                            format_asset_timeframes(&failed_list)
                                        )),
                                    ));
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
                                let _ = bot_update_tx.send(MarketUpdate::RelayToFrontend(
                                    UpdateFrontend::UserError(format!(
                                        "Failed to load candle data: {}\nIndicator changes were not applied.",
                                        e
                                    )),
                                ));
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
                        let _ = bot_update_tx.send(MarketUpdate::RelayToFrontend(
                            UpdateFrontend::UserError(format!(
                                "Failed to sync live feeds for indicator update: {}",
                                e
                            )),
                        ));
                        continue;
                    }

                    let price_data = if map.is_empty() { None } else { Some(map) };
                    let _ = engine_update_tx.send(EngineCommand::EditIndicators {
                        indicators: entry_vec.clone(),
                        price_data,
                    });
                    self.manual_indicators = next_manual_indicators;
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
    bot_cmd_tx: Sender<BotEvent>,
    engine_tx: UnboundedSender<EngineCommand>,
    exec_tx: FlumeSender<ExecCommand>,
}

struct MarketReceivers {
    pub price_rv: UnboundedReceiver<PriceAsset>,
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
