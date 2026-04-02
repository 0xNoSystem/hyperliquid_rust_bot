pub mod backtester;
pub mod candle_store;
pub mod downsample;
pub mod fetcher;
pub mod types;

pub use backtester::Backtester;
pub use candle_store::CandleStore;
pub use fetcher::{DataSource, Exchange, Fetcher, MarketType};
pub use types::{
    BacktestConfig, BacktestProgress, BacktestResult, BacktestRunRequest, BacktestSim,
    BacktestSummary, CandlePoint, EquityPoint, PnlTracker, PositionSnapshot, SnapshotReason,
};
