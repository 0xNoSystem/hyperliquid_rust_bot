use serde::Serialize;
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
}

#[derive(Debug, Clone, Serialize, FromRow)]
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
