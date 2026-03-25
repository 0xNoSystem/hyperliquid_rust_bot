use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use uuid::Uuid;

use super::fetcher::DataSource;
use crate::{EngineView, IndicatorData, OpenPositionLocal, Price, TimeFrame, TradeInfo};

pub type PnlTracker = BTreeMap<u64, f64>;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BacktestConfig {
    pub asset: String,
    pub source: DataSource,
    pub strategy_id: Uuid,
    pub resolution: TimeFrame,
    pub margin: f64,
    pub lev: usize,
    pub taker_fee_bps: u32,
    pub maker_fee_bps: u32,
    pub funding_rate_bps_per_8h: f64,
    pub start_time: u64,
    pub end_time: u64,
    pub snapshot_interval_candles: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BacktestRunRequest {
    #[serde(default)]
    pub run_id: Option<String>,
    pub config: BacktestConfig,
    pub warmup_candles: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum BacktestProgress {
    Initializing,
    LoadingCandles { loaded: u64, total: u64 },
    WarmingEngine { loaded: u64, total: u64 },
    Simulating { processed: u64, total: u64 },
    Finalizing,
    Done,
    Failed { message: String },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CandlePoint {
    pub open_time: u64,
    pub close_time: u64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

impl From<Price> for CandlePoint {
    fn from(p: Price) -> Self {
        Self {
            open_time: p.open_time,
            close_time: p.close_time,
            open: p.open,
            high: p.high,
            low: p.low,
            close: p.close,
            volume: p.vlm,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EquityPoint {
    pub ts: u64,
    pub equity: f64,
    pub balance: f64,
    pub upnl: f64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SnapshotReason {
    Open,
    Reduce,
    Flatten,
    Close,
    ForceClose,
    CancelResting,
    Fill,
    Interval,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PositionSnapshot {
    pub id: u64,
    pub ts: u64,
    pub candle: CandlePoint,
    pub upnl: f64,
    pub balance: f64,
    pub equity: f64,
    pub reason: SnapshotReason,
    pub engine_state: EngineView,
    pub indicators: Vec<IndicatorData>,
    pub position: Option<OpenPositionLocal>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BacktestSummary {
    pub initial_equity: f64,
    pub final_equity: f64,
    pub net_pnl: f64,
    pub return_pct: f64,
    pub max_drawdown_abs: f64,
    pub max_drawdown_pct: f64,
    pub total_trades: usize,
    pub wins: usize,
    pub losses: usize,
    pub win_rate_pct: f64,
    pub gross_profit: f64,
    pub gross_loss: f64,
    pub avg_win: f64,
    pub avg_loss: f64,
    pub profit_factor: Option<f64>,
    pub expectancy: f64,
    #[serde(default)]
    pub sharpe_ratio: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BacktestResult {
    pub run_id: String,
    pub started_at: u64,
    pub finished_at: u64,
    pub candles_loaded: u64,
    pub candles_processed: u64,
    pub config: BacktestConfig,
    pub summary: BacktestSummary,
    pub trades: Vec<TradeInfo>,
    pub equity_curve: Vec<EquityPoint>,
    pub snapshots: Vec<PositionSnapshot>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BacktestSim {
    pub config: BacktestConfig,
    pub pending_order_ids: Vec<u64>,
    pub position: Option<OpenPositionLocal>,
    pub trades: Vec<TradeInfo>,
    pub pnl: PnlTracker,
}
