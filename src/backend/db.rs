use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct TradeRow {
    pub id: sqlx::types::Uuid,
    pub pubkey: String,
    pub market: String,
    pub side: String,
    pub size: f64,
    pub pnl: f64,
    pub total_pnl: f64,
    pub fees: f64,
    pub funding: f64,
    pub open_time: i64,
    pub open_price: f64,
    pub open_type: String,
    pub close_time: i64,
    pub close_price: f64,
    pub close_type: String,
    pub strategy: Option<String>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct StrategySummary {
    pub id: sqlx::types::Uuid,
    pub name: String,
    pub is_active: Option<bool>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct StrategyRow {
    pub id: sqlx::types::Uuid,
    pub pubkey: String,
    pub name: String,
    pub on_idle: String,
    pub on_open: String,
    pub on_busy: String,
    pub indicators: serde_json::Value,
    pub is_active: Option<bool>,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

// ── Backtest persistence ────────────────────────────────────────────────────

/// Lightweight row from `backtest_runs` — used for history list.
#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct BacktestRunRow {
    pub id: sqlx::types::Uuid,
    pub pubkey: String,
    pub strategy_id: sqlx::types::Uuid,
    pub strategy_name: String,
    pub asset: String,
    pub resolution: String,
    pub exchange: String,
    pub market: String,
    pub margin: f64,
    pub lev: i32,
    pub start_time: i64,
    pub end_time: i64,
    // summary
    pub net_pnl: f64,
    pub return_pct: f64,
    pub max_drawdown_pct: f64,
    pub total_trades: i32,
    pub win_rate_pct: f64,
    pub profit_factor: Option<f64>,
    pub sharpe_ratio: Option<f64>,
    pub started_at: i64,
    pub finished_at: i64,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Heavy row from `backtest_results` — fetched on click.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct BacktestResultRow {
    pub id: sqlx::types::Uuid,
    pub run_id: sqlx::types::Uuid,
    pub initial_equity: f64,
    pub final_equity: f64,
    pub gross_profit: f64,
    pub gross_loss: f64,
    pub avg_win: f64,
    pub avg_loss: f64,
    pub expectancy: f64,
    pub wins: i32,
    pub losses: i32,
    pub candles_loaded: i64,
    pub candles_processed: i64,
    pub max_drawdown_abs: f64,
    pub trades: serde_json::Value,
    pub equity_curve: serde_json::Value,
    pub snapshots: serde_json::Value,
}
