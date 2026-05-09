use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use log::{info, warn};
use rhai::Engine;
use sqlx::PgPool;

use super::downsample::{cap_snapshots, lttb_equity};
use super::fetcher::{DataSource, Fetcher, RequestLimiter};
use super::types::{
    BacktestProgress, BacktestResult, BacktestRunRequest, BacktestSummary, CandlePoint,
    EquityPoint, PositionSnapshot, SnapshotReason,
};
use crate::backend::app_state::StrategyCache;
use crate::strategy::replace_self_with_asset;
use crate::{
    BtAction, BtIntent, BtOrder, CloseOrder, EngineOrder, Error, FillInfo, FillType, OpenOrder,
    OpenPosInfo, OpenPositionLocal, PositionOp, Price, Side, SignalEngine, TimeFrame, TradeInfo,
    TriggerKind, Triggers, get_time_now,
};

const FUNDING_WINDOW_MS: u64 = 8 * 60 * 60 * 1000;
const EPSILON: f64 = 1e-12;
const FETCH_WINDOW_CANDLES: u64 = 50_000;
const MAX_FETCH_WORKERS: usize = 4;
const MAX_FETCH_REQUESTS_PER_SEC: u32 = 4;

#[derive(Clone, Copy, Debug)]
struct PositionState {
    side: Side,
    size: f64,
    entry_px: f64,
    open_time: u64,
    fees: f64,
    funding: f64,
    realised_pnl: f64,
    fill_type: FillType,
}

impl PositionState {
    fn to_open_pos_info(self) -> OpenPosInfo {
        OpenPosInfo {
            side: self.side,
            size: self.size,
            entry_px: self.entry_px,
            open_time: self.open_time,
        }
    }

    fn to_open_position_local(self) -> OpenPositionLocal {
        OpenPositionLocal {
            open_time: self.open_time,
            size: self.size,
            entry_px: self.entry_px,
            side: self.side,
            fees: self.fees,
            funding: self.funding,
            realised_pnl: self.realised_pnl,
            fill_type: self.fill_type,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum RestingKind {
    Open { triggers: Option<Triggers> },
    Close,
}

#[derive(Clone, Copy, Debug)]
struct RestingOrder {
    order: EngineOrder,
    kind: RestingKind,
    placed_at: u64,
}

enum FetchWindowEvent {
    Window { idx: usize, prices: Vec<Price> },
    Done,
    Failed(String),
}

#[derive(Clone, Copy, Debug)]
struct FetchWindow {
    idx: usize,
    start: u64,
    end: u64,
}

#[derive(Clone, Copy, Debug)]
struct WorkerProgress {
    idx: usize,
    loaded: u64,
    total: u64,
}

#[derive(Debug)]
struct WorkerResult {
    idx: usize,
    result: Result<Vec<Price>, String>,
}

#[derive(Clone, Debug)]
struct BacktestSeries {
    asset: Arc<str>,
    tf: TimeFrame,
    prices: Vec<Price>,
    first_sim_idx: usize,
}

#[derive(Clone, Copy, Debug)]
struct SeriesEvent {
    series_idx: usize,
    price: Price,
}

pub struct Backtester {
    request: BacktestRunRequest,
    candle_store: Arc<super::candle_store::CandleStore>,
    engine: SignalEngine,
    required_series: Vec<(Arc<str>, TimeFrame)>,
    next_order_id: u64,
    next_snapshot_id: u64,
    balance: f64,
    position: Option<PositionState>,
    resting_orders: HashMap<u64, RestingOrder>,
    trades: Vec<TradeInfo>,
    equity_curve: Vec<EquityPoint>,
    snapshots: Vec<PositionSnapshot>,
    next_funding_time: Option<u64>,
}

impl Backtester {
    pub async fn from_request(
        request: BacktestRunRequest,
        rhai_engine: Arc<Engine>,
        strategy_cache: StrategyCache,
        pool: &PgPool,
        candle_store: Arc<super::candle_store::CandleStore>,
    ) -> Result<Self, Error> {
        let sid = request.config.strategy_id;
        let margin = request.config.margin;
        let lev = request.config.lev;
        // Cache is kept in sync on save/update/delete — prefer it over DB
        let (compiled, strat_indicators) = {
            let cached = {
                let guard = strategy_cache.read().await;
                guard.get(&sid).cloned()
            };

            if let Some(entry) = cached {
                (entry.compiled, entry.indicators)
            } else {
                // Cache miss — fetch from DB, compile, and cache
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

                {
                    let mut guard = strategy_cache.write().await;
                    guard.insert(
                        sid,
                        crate::backend::app_state::CachedStrategy {
                            compiled: compiled.clone(),
                            indicators: indicators.clone(),
                            state_declarations: state_decls,
                            name: row.name,
                        },
                    );
                }

                (compiled, indicators)
            }
        };

        let mut strat_indicators = strat_indicators;
        replace_self_with_asset(request.config.asset.as_str(), &mut strat_indicators);
        let required_series = collect_required_series(
            &strat_indicators,
            Arc::<str>::from(request.config.asset.as_str()),
            request.config.resolution,
        );

        let engine = SignalEngine::new_backtest(
            margin,
            lev,
            rhai_engine,
            compiled,
            strat_indicators,
            request.config.asset.clone().into(),
        );

        Ok(Self {
            request,
            candle_store,
            engine,
            required_series,
            next_order_id: 1,
            next_snapshot_id: 1,
            balance: margin,
            position: None,
            resting_orders: HashMap::new(),
            trades: Vec::new(),
            equity_curve: Vec::new(),
            snapshots: Vec::new(),
            next_funding_time: None,
        })
    }

    pub fn request(&self) -> &BacktestRunRequest {
        &self.request
    }

    pub async fn run(&mut self) -> Result<BacktestResult, Error> {
        self.run_with_progress(|_| {}).await
    }

    pub async fn run_with_progress<F>(
        &mut self,
        mut on_progress: F,
    ) -> Result<BacktestResult, Error>
    where
        F: FnMut(BacktestProgress),
    {
        self.reset_runtime();
        on_progress(BacktestProgress::Initializing);

        let started_at = get_time_now();
        let cfg = self.request.config.clone();
        let run_id = self
            .request
            .run_id
            .clone()
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_else(|| format!("bt-{}-{started_at}", cfg.asset));
        let execution_asset: Arc<str> = Arc::from(cfg.asset.as_str());
        let tf = cfg.resolution;
        let sim_start = cfg.start_time;
        let sim_end = cfg.end_time;
        let tf_ms = tf.to_millis();
        if tf_ms == 0 || sim_end <= sim_start {
            let err = Error::Custom("Invalid backtest range or timeframe".to_string());
            on_progress(BacktestProgress::Failed {
                message: err.to_string(),
            });
            return Err(err);
        }

        let warmup_target = self.request.warmup_candles;
        let fetch_end = sim_end;
        let fetch_total = self
            .required_series
            .iter()
            .map(|(_, series_tf)| {
                let series_tf_ms = series_tf.to_millis();
                let fetch_start =
                    sim_start.saturating_sub(warmup_target.saturating_mul(series_tf_ms));
                estimate_candle_count(fetch_start, fetch_end, series_tf_ms).max(1)
            })
            .sum::<u64>()
            .max(1);
        let sim_total = self
            .required_series
            .iter()
            .map(|(_, series_tf)| {
                estimate_candle_count(sim_start, sim_end, series_tf.to_millis()).max(1)
            })
            .sum::<u64>()
            .max(1);
        let warmup_total = warmup_target.saturating_mul(self.required_series.len() as u64);
        let loading_log_step = (fetch_total / 10).max(1);
        let mut next_loading_log = loading_log_step;
        let sim_log_step = (sim_total / 10).max(1);
        let mut next_sim_log = sim_log_step;

        info!(
            "backtest[{run_id}] start asset={} source={:?} exec_tf={:?} sim={}..{} warmup={} required_series=[{}] est_fetch={} est_sim={} workers={} rps={}",
            cfg.asset,
            cfg.source,
            tf,
            sim_start,
            sim_end,
            warmup_target,
            format_series_keys(&self.required_series),
            fetch_total,
            sim_total,
            MAX_FETCH_WORKERS,
            MAX_FETCH_REQUESTS_PER_SEC
        );

        on_progress(BacktestProgress::LoadingCandles {
            loaded: 0,
            total: fetch_total,
        });
        on_progress(BacktestProgress::WarmingEngine {
            loaded: 0,
            total: warmup_total,
        });

        let mut loading_reported = 0_u64;
        let mut loaded_candles = 0_u64;
        let mut series_data = Vec::with_capacity(self.required_series.len());
        for (series_asset, series_tf) in &self.required_series {
            let series_tf_ms = series_tf.to_millis();
            if series_tf_ms == 0 {
                let err = Error::Custom(format!(
                    "Unsupported timeframe {:?} for series {}",
                    series_tf, series_asset
                ));
                on_progress(BacktestProgress::Failed {
                    message: err.to_string(),
                });
                return Err(err);
            }

            let series_fetch_start =
                sim_start.saturating_sub(warmup_target.saturating_mul(series_tf_ms));
            let series_prices = fetch_series_history_with_progress(
                &run_id,
                cfg.source.clone(),
                Arc::clone(series_asset),
                *series_tf,
                series_fetch_start,
                fetch_end,
                loaded_candles,
                fetch_total,
                self.candle_store.clone(),
                &mut loading_reported,
                &mut next_loading_log,
                loading_log_step,
                &mut on_progress,
            )
            .await?;

            if series_prices.is_empty() {
                let err = Error::Custom(format!(
                    "Backtest requires candle data for {} {} but none was loaded",
                    series_asset, series_tf
                ));
                on_progress(BacktestProgress::Failed {
                    message: err.to_string(),
                });
                return Err(err);
            }

            let first_sim_idx = series_prices.partition_point(|price| price.open_time < sim_start);
            if *series_asset == execution_asset
                && *series_tf == tf
                && first_sim_idx >= series_prices.len()
            {
                let err = Error::Custom("No simulation candles left after warmup".to_string());
                on_progress(BacktestProgress::Failed {
                    message: err.to_string(),
                });
                return Err(err);
            }

            loaded_candles = loaded_candles.saturating_add(series_prices.len() as u64);
            series_data.push(BacktestSeries {
                asset: Arc::clone(series_asset),
                tf: *series_tf,
                prices: series_prices,
                first_sim_idx,
            });
        }

        let loaded_now = loaded_candles.min(fetch_total);
        if loaded_now > loading_reported {
            loading_reported = loaded_now;
            on_progress(BacktestProgress::LoadingCandles {
                loaded: loaded_now,
                total: fetch_total,
            });
        }
        maybe_log_milestone(
            &run_id,
            "loading",
            loading_reported,
            fetch_total,
            &mut next_loading_log,
            loading_log_step,
        );

        let Some(primary_series_idx) = series_data
            .iter()
            .position(|series| series.asset == execution_asset && series.tf == tf)
        else {
            let err = Error::Custom("Primary execution series was not prepared".to_string());
            on_progress(BacktestProgress::Failed {
                message: err.to_string(),
            });
            return Err(err);
        };

        let mut warmup_loaded = 0_u64;
        for series in &series_data {
            let warmup_start_idx = series.first_sim_idx.saturating_sub(warmup_target as usize);
            let warmup_slice = &series.prices[warmup_start_idx..series.first_sim_idx];
            if !warmup_slice.is_empty() {
                self.engine
                    .load(&series.asset, series.tf, warmup_slice.iter().copied())
                    .await;
            }
            warmup_loaded = warmup_loaded.saturating_add(warmup_slice.len() as u64);
            if warmup_loaded.is_multiple_of(100) || warmup_loaded >= warmup_total {
                on_progress(BacktestProgress::WarmingEngine {
                    loaded: warmup_loaded,
                    total: warmup_total,
                });
            }
        }
        on_progress(BacktestProgress::WarmingEngine {
            loaded: warmup_loaded,
            total: warmup_total,
        });
        on_progress(BacktestProgress::Simulating {
            processed: 0,
            total: sim_total,
        });

        let mut sim_started = false;
        let mut sim_processed = 0_u64;
        let mut last_sim_candle = series_data[primary_series_idx]
            .prices
            .get(
                series_data[primary_series_idx]
                    .first_sim_idx
                    .saturating_sub(1),
            )
            .copied();
        let mut current_execution_candle = last_sim_candle;
        let mut stopped_by_liquidation = false;
        let mut next_indices: Vec<usize> = series_data
            .iter()
            .map(|series| series.first_sim_idx)
            .collect();

        while let Some(batch) = next_event_batch(&series_data, &mut next_indices) {
            let primary_candle = batch
                .iter()
                .find_map(|event| (event.series_idx == primary_series_idx).then_some(event.price));

            if let Some(candle) = primary_candle {
                current_execution_candle = Some(candle);
                if !sim_started {
                    self.init_funding(candle.open_time);
                    info!(
                        "backtest[{run_id}] simulation started warmup_loaded={} first_sim_ts={} sim_total={}",
                        warmup_loaded, candle.open_time, sim_total
                    );
                    sim_started = true;
                }
                self.apply_funding_if_due(candle);
                self.sync_engine_position();
            }

            let Some(execution_candle) = current_execution_candle else {
                continue;
            };

            for event in batch {
                let series = &series_data[event.series_idx];
                let actions = self.engine.tick_backtest(
                    &series.asset,
                    series.tf,
                    event.price,
                    execution_candle,
                );
                self.apply_engine_actions(actions, execution_candle);
                sim_processed = sim_processed.saturating_add(1);
            }

            if let Some(candle) = primary_candle {
                let liquidated = self.process_candle(candle);
                last_sim_candle = Some(candle);
                if liquidated {
                    stopped_by_liquidation = true;
                    info!(
                        "backtest[{run_id}] liquidation stop at candle_ts={} processed={}",
                        candle.open_time, sim_processed
                    );
                    break;
                }
            }

            if sim_processed.is_multiple_of(200) || sim_processed >= sim_total {
                on_progress(BacktestProgress::Simulating {
                    processed: sim_processed,
                    total: sim_total,
                });
                maybe_log_milestone(
                    &run_id,
                    "simulating",
                    sim_processed,
                    sim_total,
                    &mut next_sim_log,
                    sim_log_step,
                );
            }
        }

        if !sim_started {
            let err = Error::Custom("No simulation candles left after warmup".to_string());
            on_progress(BacktestProgress::Failed {
                message: err.to_string(),
            });
            return Err(err);
        }

        if !stopped_by_liquidation {
            self.finalize_open_position_at_end(last_sim_candle);
        }

        on_progress(BacktestProgress::Simulating {
            processed: sim_processed,
            total: sim_total,
        });
        maybe_log_milestone(
            &run_id,
            "simulating",
            sim_processed,
            sim_total,
            &mut next_sim_log,
            sim_log_step,
        );

        on_progress(BacktestProgress::Finalizing);
        info!(
            "backtest[{run_id}] finalizing loaded={} processed={} trades={} open_position={} resting_orders={}",
            loaded_candles,
            sim_processed,
            self.trades.len(),
            self.position.is_some(),
            self.resting_orders.len()
        );

        let finished_at = get_time_now();
        let result = self.build_result(started_at, finished_at, loaded_candles, sim_processed);

        info!(
            "backtest[{run_id}] done loaded={} processed={} trades={} net_pnl={:.6} return_pct={:.4} snapshots={} equity_points={}",
            result.candles_loaded,
            result.candles_processed,
            result.summary.total_trades,
            result.summary.net_pnl,
            result.summary.return_pct,
            result.snapshots.len(),
            result.equity_curve.len()
        );
        on_progress(BacktestProgress::Done);
        Ok(result)
    }

    fn reset_runtime(&mut self) {
        self.engine.reset_for_backtest();
        self.position = None;
        self.resting_orders.clear();
        self.trades.clear();
        self.equity_curve.clear();
        self.snapshots.clear();
        self.next_order_id = 1;
        self.next_snapshot_id = 1;
        self.balance = self.request.config.margin;
        self.next_funding_time = None;
    }

    fn apply_engine_actions(&mut self, actions: Vec<BtAction>, execution_candle: Price) {
        for action in actions {
            self.apply_action(action, execution_candle);
            self.sync_engine_position();
            if let Some(reason) = snapshot_reason_from_action(action) {
                self.capture_snapshot(execution_candle, reason);
            }
        }
    }

    fn process_candle(&mut self, candle: Price) -> bool {
        let fills = self.fill_resting_orders(candle);
        if fills > 0 {
            self.sync_engine_position();
            self.capture_snapshot(candle, SnapshotReason::Fill);
        }

        if self.apply_liquidation_if_touched(candle) {
            return true;
        }

        self.sync_engine_position();
        self.push_equity_point(candle);
        false
    }

    fn apply_action(&mut self, action: BtAction, candle: Price) {
        match action {
            BtAction::Submit { order, intent } => match order {
                BtOrder::Open(open) => self.submit_open_order(open, intent, candle),
                BtOrder::Close(close) => self.submit_close_order(close, intent, candle),
            },
            BtAction::ForceTaker { order, intent } => {
                let forced_action = match order {
                    BtOrder::Open(open) => open.order.action,
                    BtOrder::Close(close) => close.order.action,
                };
                self.resting_orders.retain(|_, resting| {
                    resting.order.is_tpsl().is_some() || resting.order.action != forced_action
                });
                match order {
                    BtOrder::Open(open) => {
                        if self.position.is_none() {
                            self.submit_open_order(open, intent, candle);
                        }
                    }
                    BtOrder::Close(close) => self.submit_close_order(close, intent, candle),
                }
            }
            BtAction::CancelAllResting => {
                self.resting_orders.clear();
            }
            BtAction::ForceCloseMarket => {
                self.resting_orders.clear();
                let _ =
                    self.fill_close_at_px(None, candle.close, candle.close_time, FillType::Market);
            }
        }
    }

    fn submit_open_order(&mut self, open: OpenOrder, _intent: BtIntent, candle: Price) {
        if open.order.limit.is_none() {
            self.fill_open_at_px(
                open.order,
                candle.close,
                candle.close_time,
                FillType::Market,
            );
            if let Some(triggers) = open.triggers {
                self.attach_triggers_after_open(triggers, open.order.size, candle.close_time);
            }
            return;
        }

        let id = self.next_id();
        self.resting_orders.insert(
            id,
            RestingOrder {
                order: open.order,
                kind: RestingKind::Open {
                    triggers: open.triggers,
                },
                placed_at: candle.open_time,
            },
        );
    }

    fn submit_close_order(&mut self, close: CloseOrder, _intent: BtIntent, candle: Price) {
        if close.order.limit.is_none() {
            let _ = self.fill_close_at_px(
                Some(close.order),
                candle.close,
                candle.close_time,
                FillType::Market,
            );
            return;
        }

        let id = self.next_id();
        self.resting_orders.insert(
            id,
            RestingOrder {
                order: close.order,
                kind: RestingKind::Close,
                placed_at: candle.open_time,
            },
        );
    }

    fn fill_resting_orders(&mut self, candle: Price) -> usize {
        let ids: Vec<u64> = self.resting_orders.keys().copied().collect();
        let mut fill_count = 0usize;

        for id in ids {
            let Some(resting) = self.resting_orders.get(&id).copied() else {
                continue;
            };

            let Some(limit) = resting.order.limit else {
                continue;
            };

            // Prevent retroactive fills: newly-placed resting orders can only fill
            // from the next candle onward.
            if resting.placed_at >= candle.open_time {
                continue;
            }

            let pos_side = self.position.map(|p| p.side);
            let above = is_trigger_above_market(&resting.order, pos_side, candle.open);
            if !trigger_hit(candle, limit.limit_px, above) {
                continue;
            }
            let fill_px = trigger_fill_px(candle, limit.limit_px, above);

            let fill_type = order_fill_type(resting.order);
            match resting.kind {
                RestingKind::Open { triggers } => {
                    let _ = self.resting_orders.remove(&id);
                    self.fill_open_at_px(resting.order, fill_px, candle.open_time, fill_type);
                    if let Some(t) = triggers {
                        self.attach_triggers_after_open(t, resting.order.size, candle.open_time);
                    }
                    fill_count += 1;
                }
                RestingKind::Close => {
                    let _ = self.resting_orders.remove(&id);
                    if self
                        .fill_close_at_px(Some(resting.order), fill_px, candle.open_time, fill_type)
                        .is_some()
                    {
                        fill_count += 1;
                    }
                }
            }
        }

        fill_count
    }

    fn fill_open_at_px(&mut self, order: EngineOrder, px: f64, ts: u64, fill_type: FillType) {
        let side = match order.action {
            PositionOp::OpenLong => Side::Long,
            PositionOp::OpenShort => Side::Short,
            PositionOp::Close => return,
        };

        let size = order.size.max(0.0);
        if size <= EPSILON {
            return;
        }

        let fee = self.calc_fee(px, size, fill_type);
        self.balance -= fee;

        match self.position {
            Some(mut pos) => {
                if pos.side != side {
                    warn!("Ignoring open fill against opposite-side position");
                    return;
                }
                let old_size = pos.size;
                let new_size = old_size + size;
                if new_size <= EPSILON {
                    return;
                }
                pos.entry_px = (pos.entry_px * old_size + px * size) / new_size;
                pos.size = new_size;
                // Opening fee is immediately realized.
                pos.realised_pnl -= fee;
                pos.fees += fee;
                self.position = Some(pos);
            }
            None => {
                self.position = Some(PositionState {
                    side,
                    size,
                    entry_px: px,
                    open_time: ts,
                    fees: fee,
                    funding: 0.0,
                    // Opening fee is immediately realized.
                    realised_pnl: -fee,
                    fill_type,
                });
            }
        }
    }

    fn fill_close_at_px(
        &mut self,
        order: Option<EngineOrder>,
        px: f64,
        ts: u64,
        fill_type: FillType,
    ) -> Option<TradeInfo> {
        let mut pos = self.position?;
        let requested = order.map(|o| o.size).unwrap_or(pos.size);
        let close_size = requested.min(pos.size).max(0.0);
        if close_size <= EPSILON {
            return None;
        }

        let fee = self.calc_fee(px, close_size, fill_type);
        let price_diff = match pos.side {
            Side::Long => px - pos.entry_px,
            Side::Short => pos.entry_px - px,
        };

        let partial_pnl = price_diff * close_size;
        let net_chunk = partial_pnl - fee;
        pos.realised_pnl += net_chunk;
        pos.size -= close_size;
        pos.fees += fee;
        self.balance += net_chunk;

        if pos.size > EPSILON {
            self.position = Some(pos);
            self.reconcile_close_order_sizes(pos.size);
            return None;
        }

        let total_pnl = pos.realised_pnl + pos.funding;
        let trade = TradeInfo {
            side: pos.side,
            size: close_size,
            pnl: total_pnl,
            total_pnl,
            fees: pos.fees,
            funding: pos.funding,
            open: FillInfo {
                time: pos.open_time,
                price: pos.entry_px,
                fill_type: pos.fill_type,
            },
            close: FillInfo {
                time: ts,
                price: px,
                fill_type,
            },
            strategy: None,
        };

        self.trades.push(trade.clone());
        self.position = None;
        self.resting_orders
            .retain(|_, order| matches!(order.kind, RestingKind::Open { .. }));

        Some(trade)
    }

    fn attach_triggers_after_open(
        &mut self,
        triggers: Triggers,
        trigger_size: f64,
        placed_at: u64,
    ) {
        let Some(open_pos) = self.position else {
            return;
        };
        let side = open_pos.side;
        let ref_px = open_pos.entry_px;
        let lev = self.request.config.lev;

        if let Some(tp_delta) = triggers.tp {
            let trigger_px = calc_trigger_px(side, TriggerKind::Tp, tp_delta, ref_px, lev);
            let order = EngineOrder::new_tp(trigger_size, trigger_px);
            let id = self.next_id();
            self.resting_orders.insert(
                id,
                RestingOrder {
                    order,
                    kind: RestingKind::Close,
                    placed_at,
                },
            );
        }

        if let Some(sl_delta) = triggers.sl {
            let trigger_px = calc_trigger_px(side, TriggerKind::Sl, sl_delta, ref_px, lev);
            let order = EngineOrder::new_sl(trigger_size, trigger_px);
            let id = self.next_id();
            self.resting_orders.insert(
                id,
                RestingOrder {
                    order,
                    kind: RestingKind::Close,
                    placed_at,
                },
            );
        }
    }

    fn reconcile_close_order_sizes(&mut self, max_size: f64) {
        for order in self.resting_orders.values_mut() {
            if matches!(order.kind, RestingKind::Close) {
                order.order.size = order.order.size.min(max_size);
            }
        }
    }

    fn sync_engine_position(&mut self) {
        let open_pos = self.position.map(|p| p.to_open_pos_info());
        self.engine.set_backtest_open_position(open_pos);
        self.engine.set_backtest_margin(self.balance.max(0.0));
    }

    fn apply_funding_if_due(&mut self, candle: Price) {
        let rate_bps = self.request.config.funding_rate_bps_per_8h;
        let mut next = match self.next_funding_time {
            Some(ts) => ts,
            None => return,
        };

        while candle.open_time >= next {
            if let Some(mut pos) = self.position
                && rate_bps != 0.0
            {
                let rate = rate_bps / 10_000.0;
                let notional = pos.size * candle.open;
                let signed = match pos.side {
                    Side::Long => -1.0,
                    Side::Short => 1.0,
                };
                let funding = notional * rate * signed;
                pos.funding += funding;
                self.balance += funding;
                self.position = Some(pos);
            }
            next = next.saturating_add(FUNDING_WINDOW_MS);
        }
        self.next_funding_time = Some(next);
    }

    fn init_funding(&mut self, first_ts: u64) {
        self.next_funding_time = Some(next_time_boundary(first_ts, FUNDING_WINDOW_MS));
    }

    fn push_equity_point(&mut self, candle: Price) {
        let upnl = self.unrealised_pnl(candle.close);
        let equity = self.balance + upnl;
        self.equity_curve.push(EquityPoint {
            ts: candle.close_time,
            equity,
            balance: self.balance,
            upnl,
        });
    }

    fn capture_snapshot(&mut self, candle: Price, reason: SnapshotReason) {
        let upnl = self.unrealised_pnl(candle.close);
        let equity = self.balance + upnl;
        let snapshot = PositionSnapshot {
            id: self.next_snapshot_id,
            ts: candle.open_time,
            candle: CandlePoint::from(candle),
            upnl,
            balance: self.balance,
            equity,
            reason,
            engine_state: self.engine.view(),
            indicators: self.engine.get_indicators_data(),
            position: self.position.map(|p| p.to_open_position_local()),
        };
        self.next_snapshot_id = self.next_snapshot_id.saturating_add(1);
        self.snapshots.push(snapshot);
    }

    fn unrealised_pnl(&self, mark_px: f64) -> f64 {
        let Some(pos) = self.position else {
            return 0.0;
        };

        let diff = match pos.side {
            Side::Long => mark_px - pos.entry_px,
            Side::Short => pos.entry_px - mark_px,
        };
        diff * pos.size
    }

    fn calc_fee(&self, px: f64, size: f64, fill_type: FillType) -> f64 {
        let bps = match fill_type {
            FillType::Market | FillType::Liquidation => self.request.config.taker_fee_bps as f64,
            FillType::Limit => self.request.config.maker_fee_bps as f64,
            FillType::Trigger(kind) => match kind {
                TriggerKind::Tp => self.request.config.maker_fee_bps as f64,
                TriggerKind::Sl => self.request.config.taker_fee_bps as f64,
            },
        };
        (px * size) * (bps / 10_000.0)
    }

    fn liquidation_price(&self) -> Option<f64> {
        let pos = self.position?;
        if pos.size <= EPSILON || pos.entry_px <= 0.0 {
            return None;
        }

        let fee_rate = self.request.config.taker_fee_bps as f64 / 10_000.0;
        let price = match pos.side {
            Side::Long => {
                let denom = 1.0 - fee_rate;
                if denom <= EPSILON {
                    return None;
                }
                (pos.entry_px - (self.balance / pos.size)) / denom
            }
            Side::Short => {
                let denom = 1.0 + fee_rate;
                if denom <= EPSILON {
                    return None;
                }
                (pos.entry_px + (self.balance / pos.size)) / denom
            }
        };

        (price.is_finite() && price > 0.0).then_some(price)
    }

    fn apply_liquidation_if_touched(&mut self, candle: Price) -> bool {
        let Some(pos) = self.position else {
            return false;
        };
        let Some(liq_px) = self.liquidation_price() else {
            return false;
        };

        let touched = match pos.side {
            Side::Long => candle.low <= liq_px,
            Side::Short => candle.high >= liq_px,
        };
        if !touched {
            return false;
        }

        self.resting_orders.clear();
        if self
            .fill_close_at_px(None, liq_px, candle.open_time, FillType::Liquidation)
            .is_none()
        {
            return false;
        }

        if self.balance < 0.0 {
            self.balance = 0.0;
        }
        self.sync_engine_position();
        self.capture_snapshot(candle, SnapshotReason::ForceClose);
        self.push_equity_point(candle);
        info!(
            "backtest liquidation fill ts={} side={:?} liq_px={:.6}",
            candle.open_time, pos.side, liq_px
        );
        true
    }

    fn finalize_open_position_at_end(&mut self, last_candle: Option<Price>) {
        let Some(candle) = last_candle else {
            return;
        };
        if self.position.is_none() {
            self.resting_orders.clear();
            return;
        }

        let close_px = candle.close;
        let close_ts = candle.close_time;
        if self
            .fill_close_at_px(None, close_px, close_ts, FillType::Market)
            .is_some()
        {
            self.sync_engine_position();
            self.capture_snapshot(candle, SnapshotReason::ForceClose);
            self.push_equity_point(candle);
            info!(
                "backtest forced end-of-run close ts={} px={:.6} remaining_resting_orders={}",
                close_ts,
                close_px,
                self.resting_orders.len()
            );
        } else {
            warn!("backtest end-of-run close was expected but did not execute");
        }
        self.resting_orders.clear();
    }

    fn build_result(
        &self,
        started_at: u64,
        finished_at: u64,
        candles_loaded: u64,
        candles_processed: u64,
    ) -> BacktestResult {
        let summary = build_summary(
            self.request.config.margin,
            self.equity_curve
                .last()
                .map(|p| p.equity)
                .unwrap_or(self.balance),
            &self.equity_curve,
            &self.trades,
            self.request.config.resolution,
        );
        let closed_trade_pnl = self.trades.iter().map(|t| t.total_pnl).sum::<f64>();
        if (summary.net_pnl - closed_trade_pnl).abs() > 1e-6 {
            warn!(
                "backtest summary mismatch net_pnl={:.6} closed_trade_pnl={:.6} open_position={}",
                summary.net_pnl,
                closed_trade_pnl,
                self.position.is_some()
            );
        }

        let equity_curve = lttb_equity(&self.equity_curve, self.request.config.max_equity_points);
        let snapshots = cap_snapshots(&self.snapshots, self.request.config.max_snapshots);

        BacktestResult {
            run_id: format!("bt-{}-{}", self.request.config.asset, started_at),
            started_at,
            finished_at,
            candles_loaded,
            candles_processed,
            config: self.request.config.clone(),
            summary,
            trades: self.trades.clone(),
            equity_curve,
            snapshots,
        }
    }

    fn next_id(&mut self) -> u64 {
        let id = self.next_order_id;
        self.next_order_id = self.next_order_id.saturating_add(1);
        id
    }
}

fn collect_required_series(
    indicators: &[crate::IndexId],
    execution_asset: Arc<str>,
    execution_tf: TimeFrame,
) -> Vec<(Arc<str>, TimeFrame)> {
    let execution_key = (Arc::clone(&execution_asset), execution_tf);
    let mut seen: HashSet<(Arc<str>, TimeFrame)> = HashSet::from([execution_key.clone()]);
    let mut out = vec![execution_key.clone()];

    for (asset, _, tf) in indicators {
        let key = (Arc::clone(asset), *tf);
        if seen.insert(key.clone()) {
            out.push(key);
        }
    }

    out.sort_by(|a, b| {
        let a_primary = *a == execution_key;
        let b_primary = *b == execution_key;
        match (a_primary, b_primary) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => {
                a.0.as_ref()
                    .cmp(b.0.as_ref())
                    .then_with(|| a.1.as_str().cmp(b.1.as_str()))
            }
        }
    });

    out
}

fn format_series_keys(series: &[(Arc<str>, TimeFrame)]) -> String {
    series
        .iter()
        .map(|(asset, tf)| format!("{asset}@{}", tf.as_str()))
        .collect::<Vec<_>>()
        .join(", ")
}

#[allow(clippy::too_many_arguments)]
async fn stream_fetch_windows_parallel(
    run_id: String,
    source: DataSource,
    asset: String,
    tf: TimeFrame,
    fetch_start: u64,
    fetch_end: u64,
    window_span_ms: u64,
    fetch_total: u64,
    max_workers: usize,
    loading_tx: tokio::sync::mpsc::UnboundedSender<(u64, u64)>,
    window_tx: tokio::sync::mpsc::Sender<FetchWindowEvent>,
    candle_store: Arc<super::candle_store::CandleStore>,
) {
    let windows = build_fetch_windows(fetch_start, fetch_end, window_span_ms);
    if windows.is_empty() {
        info!("backtest[{run_id}] fetch pipeline has no windows");
        let _ = window_tx.send(FetchWindowEvent::Done).await;
        return;
    }

    let request_limiter = RequestLimiter::from_requests_per_second(MAX_FETCH_REQUESTS_PER_SEC);
    let worker_count = max_workers.clamp(1, MAX_FETCH_WORKERS).min(windows.len());
    info!(
        "backtest[{run_id}] fetch pipeline start windows={} workers={} span_ms={}",
        windows.len(),
        worker_count,
        window_span_ms
    );
    let (progress_tx, mut progress_rx) = tokio::sync::mpsc::unbounded_channel::<WorkerProgress>();
    let mut joinset = tokio::task::JoinSet::<WorkerResult>::new();

    let mut per_window_loaded = vec![0_u64; windows.len()];
    let mut global_loaded = 0_u64;
    let mut next_spawn = 0usize;
    let mut completed = 0usize;

    while next_spawn < windows.len() && joinset.len() < worker_count {
        spawn_fetch_worker(
            &mut joinset,
            windows[next_spawn],
            source.clone(),
            asset.clone(),
            tf,
            request_limiter.clone(),
            progress_tx.clone(),
            run_id.clone(),
            candle_store.clone(),
        );
        next_spawn += 1;
    }

    while completed < windows.len() {
        tokio::select! {
            maybe_progress = progress_rx.recv() => {
                if let Some(progress) = maybe_progress
                    && progress.idx < per_window_loaded.len()
                {
                    let loaded = progress.loaded.min(progress.total);
                    if loaded > per_window_loaded[progress.idx] {
                        global_loaded = global_loaded
                            .saturating_add(loaded - per_window_loaded[progress.idx]);
                        per_window_loaded[progress.idx] = loaded;
                        let _ = loading_tx.send((global_loaded.min(fetch_total), fetch_total));
                    }
                }
            }
            join_result = joinset.join_next() => {
                let Some(join_result) = join_result else {
                    break;
                };

                let worker_result = match join_result {
                    Ok(worker_result) => worker_result,
                    Err(e) => {
                        warn!("backtest[{run_id}] fetch worker join failed: {e}");
                        let _ = window_tx
                            .send(FetchWindowEvent::Failed(format!(
                                "Backtest fetch worker join failed: {e}"
                            )))
                            .await;
                        return;
                    }
                };
                completed = completed.saturating_add(1);
                if completed.is_multiple_of(10) || completed == windows.len() {
                    info!(
                        "backtest[{run_id}] fetch windows completed {}/{}",
                        completed,
                        windows.len()
                    );
                }

                match worker_result.result {
                    Ok(prices) => {
                        let observed = prices.len() as u64;
                        if worker_result.idx < per_window_loaded.len()
                            && observed > per_window_loaded[worker_result.idx]
                        {
                            global_loaded = global_loaded.saturating_add(
                                observed - per_window_loaded[worker_result.idx]
                            );
                            per_window_loaded[worker_result.idx] = observed;
                            let _ = loading_tx.send((global_loaded.min(fetch_total), fetch_total));
                        }

                        if window_tx
                            .send(FetchWindowEvent::Window {
                                idx: worker_result.idx,
                                prices,
                            })
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
                    Err(message) => {
                        warn!(
                            "backtest[{run_id}] worker {} failed: {}",
                            worker_result.idx,
                            message
                        );
                        let _ = window_tx.send(FetchWindowEvent::Failed(message)).await;
                        return;
                    }
                }

                while next_spawn < windows.len() && joinset.len() < worker_count {
                    spawn_fetch_worker(
                        &mut joinset,
                        windows[next_spawn],
                        source.clone(),
                        asset.clone(),
                        tf,
                        request_limiter.clone(),
                        progress_tx.clone(),
                        run_id.clone(),
                        candle_store.clone(),
                    );
                    next_spawn += 1;
                }
            }
        }
    }

    while let Ok(progress) = progress_rx.try_recv() {
        if progress.idx >= per_window_loaded.len() {
            continue;
        }
        let loaded = progress.loaded.min(progress.total);
        if loaded > per_window_loaded[progress.idx] {
            global_loaded = global_loaded.saturating_add(loaded - per_window_loaded[progress.idx]);
            per_window_loaded[progress.idx] = loaded;
            let _ = loading_tx.send((global_loaded.min(fetch_total), fetch_total));
        }
    }

    let _ = loading_tx.send((global_loaded.min(fetch_total), fetch_total));
    info!(
        "backtest[{run_id}] fetch pipeline complete loaded={}/{}",
        global_loaded.min(fetch_total),
        fetch_total
    );
    let _ = window_tx.send(FetchWindowEvent::Done).await;
}

fn build_fetch_windows(fetch_start: u64, fetch_end: u64, window_span_ms: u64) -> Vec<FetchWindow> {
    let mut windows = Vec::new();
    let mut cursor = fetch_start;
    let mut idx = 0usize;

    while cursor < fetch_end {
        let mut end = cursor.saturating_add(window_span_ms);
        if end > fetch_end || end <= cursor {
            end = fetch_end;
        }
        windows.push(FetchWindow {
            idx,
            start: cursor,
            end,
        });
        idx = idx.saturating_add(1);
        cursor = end;
    }

    windows
}

#[allow(clippy::too_many_arguments)]
fn spawn_fetch_worker(
    joinset: &mut tokio::task::JoinSet<WorkerResult>,
    window: FetchWindow,
    source: DataSource,
    asset: String,
    tf: TimeFrame,
    request_limiter: Option<RequestLimiter>,
    progress_tx: tokio::sync::mpsc::UnboundedSender<WorkerProgress>,
    run_id: String,
    candle_store: Arc<super::candle_store::CandleStore>,
) {
    joinset.spawn(async move {
        fetch_window_worker(
            window,
            source,
            asset,
            tf,
            request_limiter,
            progress_tx,
            run_id,
            candle_store,
        )
        .await
    });
}

#[allow(clippy::too_many_arguments)]
async fn fetch_window_worker(
    window: FetchWindow,
    source: DataSource,
    asset: String,
    tf: TimeFrame,
    request_limiter: Option<RequestLimiter>,
    progress_tx: tokio::sync::mpsc::UnboundedSender<WorkerProgress>,
    run_id: String,
    candle_store: Arc<super::candle_store::CandleStore>,
) -> WorkerResult {
    let mut fetcher = Fetcher::new(source, candle_store);
    fetcher.set_request_limiter(request_limiter);
    let result = fetcher
        .fetch_with_progress(&asset, tf, window.start, window.end, |loaded, total| {
            let _ = progress_tx.send(WorkerProgress {
                idx: window.idx,
                loaded,
                total,
            });
        })
        .await
        .map_err(|e| {
            let message = e.to_string();
            warn!(
                "backtest[{run_id}] fetch window {} failed range={}..{}: {}",
                window.idx, window.start, window.end, message
            );
            message
        });

    WorkerResult {
        idx: window.idx,
        result,
    }
}

#[allow(clippy::too_many_arguments)]
async fn fetch_series_history_with_progress<F>(
    run_id: &str,
    source: DataSource,
    asset: Arc<str>,
    tf: TimeFrame,
    fetch_start: u64,
    fetch_end: u64,
    loaded_offset: u64,
    global_total: u64,
    candle_store: Arc<super::candle_store::CandleStore>,
    loading_reported: &mut u64,
    next_loading_log: &mut u64,
    loading_log_step: u64,
    on_progress: &mut F,
) -> Result<Vec<Price>, Error>
where
    F: FnMut(BacktestProgress),
{
    let tf_ms = tf.to_millis();
    let series_total = estimate_candle_count(fetch_start, fetch_end, tf_ms).max(1);
    let window_span_ms = FETCH_WINDOW_CANDLES.max(1).saturating_mul(tf_ms).max(tf_ms);
    let estimated_windows =
        div_ceil_u64(fetch_end.saturating_sub(fetch_start), window_span_ms).max(1);
    info!(
        "backtest[{run_id}] fetch series asset={} tf={:?} range={}..{} est_total={} windows={}",
        asset, tf, fetch_start, fetch_end, series_total, estimated_windows
    );

    let (loading_tx, mut loading_rx) = tokio::sync::mpsc::unbounded_channel::<(u64, u64)>();
    let (window_tx, mut window_rx) = tokio::sync::mpsc::channel::<FetchWindowEvent>(2);
    let producer = tokio::spawn(stream_fetch_windows_parallel(
        run_id.to_string(),
        source,
        asset.to_string(),
        tf,
        fetch_start,
        fetch_end,
        window_span_ms,
        series_total,
        MAX_FETCH_WORKERS,
        loading_tx,
        window_tx,
        candle_store,
    ));

    let mut out = Vec::new();
    let mut fatal_error: Option<Error> = None;
    let mut pending_windows: BTreeMap<usize, Vec<Price>> = BTreeMap::new();
    let mut next_window_idx = 0usize;
    let mut last_seen_open_time: Option<u64> = None;
    let mut producer_done = false;

    while !producer_done {
        tokio::select! {
            maybe_loading = loading_rx.recv(), if !loading_rx.is_closed() => {
                if let Some((loaded, total)) = maybe_loading {
                    let global_loaded = loaded_offset
                        .saturating_add(loaded.min(total))
                        .min(global_total);
                    if global_loaded > *loading_reported {
                        *loading_reported = global_loaded;
                        on_progress(BacktestProgress::LoadingCandles {
                            loaded: global_loaded,
                            total: global_total,
                        });
                        maybe_log_milestone(
                            run_id,
                            "loading",
                            global_loaded,
                            global_total,
                            next_loading_log,
                            loading_log_step,
                        );
                    }
                }
            }
            maybe_event = window_rx.recv() => {
                let Some(event) = maybe_event else {
                    fatal_error = Some(Error::Custom(
                        "Backtest fetch pipeline closed unexpectedly".to_string(),
                    ));
                    break;
                };

                match event {
                    FetchWindowEvent::Window { idx, prices } => {
                        pending_windows.insert(idx, prices);
                        while let Some(prices) = pending_windows.remove(&next_window_idx) {
                            for candle in prices {
                                if candle.open_time < fetch_start || candle.open_time >= fetch_end {
                                    continue;
                                }
                                if let Some(last_ts) = last_seen_open_time
                                    && candle.open_time <= last_ts
                                {
                                    continue;
                                }
                                last_seen_open_time = Some(candle.open_time);
                                out.push(candle);
                            }
                            next_window_idx = next_window_idx.saturating_add(1);
                        }
                    }
                    FetchWindowEvent::Done => producer_done = true,
                    FetchWindowEvent::Failed(message) => {
                        fatal_error = Some(Error::Custom(message));
                        producer_done = true;
                    }
                }
            }
        }
    }

    while let Ok((loaded, total)) = loading_rx.try_recv() {
        let global_loaded = loaded_offset
            .saturating_add(loaded.min(total))
            .min(global_total);
        if global_loaded > *loading_reported {
            *loading_reported = global_loaded;
            on_progress(BacktestProgress::LoadingCandles {
                loaded: global_loaded,
                total: global_total,
            });
            maybe_log_milestone(
                run_id,
                "loading",
                global_loaded,
                global_total,
                next_loading_log,
                loading_log_step,
            );
        }
    }

    if !pending_windows.is_empty() && fatal_error.is_none() {
        fatal_error = Some(Error::Custom(
            "Backtest fetch pipeline ended with unresolved window ordering".to_string(),
        ));
    }

    if let Err(e) = producer.await {
        let err = Error::Custom(format!("Backtest fetch task join failed: {e}"));
        warn!("backtest[{run_id}] fetch task join failed: {e}");
        return Err(err);
    }

    if let Some(err) = fatal_error {
        warn!(
            "backtest[{run_id}] failed while fetching {asset} {:?}: {err}",
            tf
        );
        return Err(err);
    }

    let final_loaded = loaded_offset
        .saturating_add(out.len() as u64)
        .min(global_total);
    if final_loaded > *loading_reported {
        *loading_reported = final_loaded;
        on_progress(BacktestProgress::LoadingCandles {
            loaded: final_loaded,
            total: global_total,
        });
        maybe_log_milestone(
            run_id,
            "loading",
            final_loaded,
            global_total,
            next_loading_log,
            loading_log_step,
        );
    }

    Ok(out)
}

fn next_event_batch(
    series_data: &[BacktestSeries],
    next_indices: &mut [usize],
) -> Option<Vec<SeriesEvent>> {
    let next_ts = series_data
        .iter()
        .enumerate()
        .filter_map(|(idx, series)| {
            series
                .prices
                .get(next_indices[idx])
                .map(|price| price.open_time)
        })
        .min()?;

    let mut batch = Vec::new();
    for (idx, series) in series_data.iter().enumerate() {
        let Some(price) = series.prices.get(next_indices[idx]).copied() else {
            continue;
        };
        if price.open_time != next_ts {
            continue;
        }
        batch.push(SeriesEvent {
            series_idx: idx,
            price,
        });
        next_indices[idx] = next_indices[idx].saturating_add(1);
    }

    batch.sort_by(|a, b| {
        let series_a = &series_data[a.series_idx];
        let series_b = &series_data[b.series_idx];
        series_a
            .asset
            .as_ref()
            .cmp(series_b.asset.as_ref())
            .then_with(|| series_a.tf.as_str().cmp(series_b.tf.as_str()))
    });
    Some(batch)
}

fn maybe_log_milestone(
    run_id: &str,
    stage: &str,
    current: u64,
    total: u64,
    next_threshold: &mut u64,
    step: u64,
) {
    if total == 0 || step == 0 {
        return;
    }
    if current < *next_threshold && current < total {
        return;
    }

    info!("backtest[{run_id}] {stage} progress {current}/{total}");

    while *next_threshold <= current {
        *next_threshold = next_threshold.saturating_add(step);
        if *next_threshold == 0 {
            break;
        }
    }
}

fn build_summary(
    initial_equity: f64,
    final_equity: f64,
    equity_curve: &[EquityPoint],
    trades: &[TradeInfo],
    resolution: TimeFrame,
) -> BacktestSummary {
    let net_pnl = final_equity - initial_equity;
    let return_pct = if initial_equity.abs() > EPSILON {
        (net_pnl / initial_equity) * 100.0
    } else {
        0.0
    };

    let mut peak = initial_equity;
    let mut max_drawdown_abs = 0.0;
    let mut max_drawdown_pct = 0.0;
    for point in equity_curve {
        if point.equity > peak {
            peak = point.equity;
        }
        let dd = (peak - point.equity).max(0.0);
        if dd > max_drawdown_abs {
            max_drawdown_abs = dd;
        }
        if peak > EPSILON {
            let dd_pct = (dd / peak) * 100.0;
            if dd_pct > max_drawdown_pct {
                max_drawdown_pct = dd_pct;
            }
        }
    }

    let total_trades = trades.len();
    let wins = trades.iter().filter(|t| t.pnl > 0.0).count();
    let losses = trades.iter().filter(|t| t.pnl < 0.0).count();
    let win_rate_pct = if total_trades > 0 {
        (wins as f64 / total_trades as f64) * 100.0
    } else {
        0.0
    };

    let gross_profit: f64 = trades.iter().filter(|t| t.pnl > 0.0).map(|t| t.pnl).sum();
    let gross_loss_abs: f64 = trades
        .iter()
        .filter(|t| t.pnl < 0.0)
        .map(|t| t.pnl.abs())
        .sum();
    let avg_win = if wins > 0 {
        gross_profit / wins as f64
    } else {
        0.0
    };
    let avg_loss = if losses > 0 {
        gross_loss_abs / losses as f64
    } else {
        0.0
    };
    let profit_factor = if gross_loss_abs > EPSILON {
        Some(gross_profit / gross_loss_abs)
    } else {
        None
    };
    let expectancy = if total_trades > 0 {
        trades.iter().map(|t| t.pnl).sum::<f64>() / total_trades as f64
    } else {
        0.0
    };
    let sharpe_ratio = compute_sharpe_ratio(initial_equity, equity_curve, resolution);

    BacktestSummary {
        initial_equity,
        final_equity,
        net_pnl,
        return_pct,
        max_drawdown_abs,
        max_drawdown_pct,
        total_trades,
        wins,
        losses,
        win_rate_pct,
        gross_profit,
        gross_loss: gross_loss_abs,
        avg_win,
        avg_loss,
        profit_factor,
        expectancy,
        sharpe_ratio,
    }
}

fn compute_sharpe_ratio(
    _initial_equity: f64,
    equity_curve: &[EquityPoint],
    resolution: TimeFrame,
) -> Option<f64> {
    if equity_curve.len() < 2 {
        return None;
    }

    // Keep one point per timestamp (latest point wins) to avoid duplicate-time noise.
    let mut points: Vec<(u64, f64)> = Vec::with_capacity(equity_curve.len());
    for point in equity_curve {
        match points.last_mut() {
            Some((last_ts, last_equity)) if *last_ts == point.ts => {
                *last_equity = point.equity;
            }
            _ => points.push((point.ts, point.equity)),
        }
    }

    if points.len() < 2 {
        return None;
    }

    let mut returns: Vec<f64> = Vec::with_capacity(points.len().saturating_sub(1));
    let mut delta_secs: Vec<f64> = Vec::with_capacity(points.len().saturating_sub(1));

    for w in points.windows(2) {
        let (prev_ts, prev_equity) = w[0];
        let (ts, equity) = w[1];

        if prev_equity.abs() <= EPSILON {
            continue;
        }

        let dt_ms = ts.saturating_sub(prev_ts);
        if dt_ms == 0 {
            continue;
        }

        let ret = (equity - prev_equity) / prev_equity;
        if ret.is_finite() {
            returns.push(ret);
            delta_secs.push((dt_ms as f64) / 1000.0);
        }
    }

    if returns.len() < 2 {
        return None;
    }

    let n = returns.len() as f64;
    let mean = returns.iter().sum::<f64>() / n;
    let variance = returns
        .iter()
        .map(|r| {
            let d = *r - mean;
            d * d
        })
        .sum::<f64>()
        / (n - 1.0);
    if !variance.is_finite() || variance <= EPSILON {
        return None;
    }

    let std_dev = variance.sqrt();

    // Annualize by observed sampling cadence (fallback to base resolution if needed).
    let mut sorted_dt = delta_secs;
    sorted_dt.sort_by(|a, b| a.total_cmp(b));
    let median_dt_secs = if sorted_dt.len() % 2 == 1 {
        sorted_dt[sorted_dt.len() / 2]
    } else {
        let upper = sorted_dt.len() / 2;
        (sorted_dt[upper - 1] + sorted_dt[upper]) / 2.0
    };

    let fallback_dt = resolution.to_secs() as f64;
    let sample_dt_secs = if median_dt_secs > 0.0 {
        median_dt_secs
    } else {
        fallback_dt
    };
    if !sample_dt_secs.is_finite() || sample_dt_secs <= 0.0 {
        return None;
    }

    let periods_per_year = (365.0 * 24.0 * 60.0 * 60.0) / sample_dt_secs;
    if !periods_per_year.is_finite() || periods_per_year <= 0.0 {
        return None;
    }

    let sharpe = (mean / std_dev) * periods_per_year.sqrt();
    sharpe.is_finite().then_some(sharpe)
}

fn order_fill_type(order: EngineOrder) -> FillType {
    if let Some(trigger) = order.is_tpsl() {
        FillType::Trigger(trigger)
    } else if order.limit.is_some() {
        FillType::Limit
    } else {
        FillType::Market
    }
}

/// Check whether a resting order at `limit_px` is triggered by this candle.
/// `is_above_market`: true when the trigger sits above current price (Long TP, Short SL).
fn trigger_hit(candle: Price, limit_px: f64, is_above_market: bool) -> bool {
    if is_above_market {
        candle.high >= limit_px
    } else {
        candle.low <= limit_px
    }
}

/// Fill price accounting for gap-through. If candle opens past the trigger, fill at open (slippage).
fn trigger_fill_px(candle: Price, limit_px: f64, is_above_market: bool) -> f64 {
    if is_above_market {
        if candle.open >= limit_px {
            candle.open
        } else {
            limit_px
        }
    } else {
        if candle.open <= limit_px {
            candle.open
        } else {
            limit_px
        }
    }
}

/// Determine whether a resting order's trigger sits above the current market.
fn is_trigger_above_market(order: &EngineOrder, pos_side: Option<Side>, candle_open: f64) -> bool {
    if let Some(tk) = order.is_tpsl()
        && let Some(side) = pos_side
    {
        return match (side, tk) {
            (Side::Long, TriggerKind::Tp) => true,
            (Side::Long, TriggerKind::Sl) => false,
            (Side::Short, TriggerKind::Tp) => false,
            (Side::Short, TriggerKind::Sl) => true,
        };
    }
    // Fallback for regular limit orders: compare to candle open
    order.limit.is_some_and(|l| l.limit_px >= candle_open)
}

fn calc_trigger_px(side: Side, trigger: TriggerKind, delta: f64, ref_px: f64, lev: usize) -> f64 {
    if lev == 0 || ref_px <= 0.0 {
        return ref_px;
    }

    let px_delta = (delta / lev as f64) / 100.0;
    match (side, trigger) {
        (Side::Long, TriggerKind::Tp) => ref_px * (1.0 + px_delta),
        (Side::Short, TriggerKind::Tp) => ref_px * (1.0 - px_delta),
        (Side::Long, TriggerKind::Sl) => ref_px * (1.0 - px_delta),
        (Side::Short, TriggerKind::Sl) => ref_px * (1.0 + px_delta),
    }
}

fn next_time_boundary(ts: u64, step_ms: u64) -> u64 {
    if step_ms == 0 {
        return ts;
    }
    let rem = ts % step_ms;
    if rem == 0 { ts } else { ts + (step_ms - rem) }
}

fn estimate_candle_count(start: u64, end: u64, step_ms: u64) -> u64 {
    if end <= start || step_ms == 0 {
        return 0;
    }
    div_ceil_u64(end - start, step_ms)
}

fn div_ceil_u64(value: u64, divisor: u64) -> u64 {
    if divisor == 0 {
        return 0;
    }
    value / divisor + u64::from(!value.is_multiple_of(divisor))
}

fn snapshot_reason_from_action(action: BtAction) -> Option<SnapshotReason> {
    match action {
        BtAction::Submit { intent, .. } | BtAction::ForceTaker { intent, .. } => {
            Some(match intent {
                BtIntent::Open => SnapshotReason::Open,
                BtIntent::Reduce => SnapshotReason::Reduce,
                BtIntent::Flatten => SnapshotReason::Flatten,
            })
        }
        BtAction::CancelAllResting => Some(SnapshotReason::CancelResting),
        BtAction::ForceCloseMarket => Some(SnapshotReason::ForceClose),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{BacktestSeries, SeriesEvent, collect_required_series, next_event_batch};
    use crate::{IndicatorKind, Price, TimeFrame};

    fn price(ts: u64, close: f64) -> Price {
        Price {
            open_time: ts,
            close_time: ts + 60_000,
            open: close,
            high: close,
            low: close,
            close,
            vlm: 1.0,
        }
    }

    #[test]
    fn collect_required_series_includes_execution_series_first() {
        let execution_asset = Arc::<str>::from("BTC");
        let series = collect_required_series(
            &[
                (
                    Arc::<str>::from("BTC"),
                    IndicatorKind::Rsi(14),
                    TimeFrame::Hour1,
                ),
                (
                    Arc::<str>::from("SOL"),
                    IndicatorKind::Ema(9),
                    TimeFrame::Min15,
                ),
                (
                    Arc::<str>::from("SOL"),
                    IndicatorKind::Rsi(7),
                    TimeFrame::Min15,
                ),
            ],
            Arc::clone(&execution_asset),
            TimeFrame::Min1,
        );

        assert_eq!(series[0], (execution_asset, TimeFrame::Min1));
        assert!(series.contains(&(Arc::<str>::from("BTC"), TimeFrame::Hour1)));
        assert!(series.contains(&(Arc::<str>::from("SOL"), TimeFrame::Min15)));
        assert_eq!(series.len(), 3);
    }

    #[test]
    fn next_event_batch_merges_matching_timestamps() {
        let series_data = vec![
            BacktestSeries {
                asset: Arc::<str>::from("BTC"),
                tf: TimeFrame::Min1,
                prices: vec![price(60_000, 100.0), price(120_000, 101.0)],
                first_sim_idx: 0,
            },
            BacktestSeries {
                asset: Arc::<str>::from("SOL"),
                tf: TimeFrame::Min15,
                prices: vec![price(60_000, 20.0), price(180_000, 21.0)],
                first_sim_idx: 0,
            },
        ];
        let mut next_indices = vec![0, 0];

        let first = next_event_batch(&series_data, &mut next_indices).expect("first batch");
        assert_eq!(
            first
                .iter()
                .map(|event: &SeriesEvent| event.price.open_time)
                .collect::<Vec<_>>(),
            vec![60_000, 60_000]
        );

        let second = next_event_batch(&series_data, &mut next_indices).expect("second batch");
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].price.open_time, 120_000);

        let third = next_event_batch(&series_data, &mut next_indices).expect("third batch");
        assert_eq!(third.len(), 1);
        assert_eq!(third[0].price.open_time, 180_000);
    }
}
