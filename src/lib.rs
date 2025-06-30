mod market;
mod executor;
mod consts;
mod wallet;
// mod backtest; 


pub mod helper;
pub mod signal;
pub mod strategy;
pub mod trade_setup;
pub mod bot;
pub mod margin;

pub use bot::{Bot, BotEvent, BotToMarket,AddMarketInfo, UpdateFrontend};
pub use wallet::Wallet;
pub use signal::{SignalEngine, IndexId, IndicatorKind, EditType, Entry};
pub use market::{Market, MarketCommand, MarketUpdate, AssetPrice};
pub use consts::{MAX_HISTORY, MARKETS};
pub use executor::Executor;
// pub use backtest::BackTester; 
pub use trade_setup::{TradeParams, TimeFrame, TradeCommand, TradeInfo, TradeFillInfo, LiquidationFillInfo};
pub use margin::{AssetMargin, MarginAllocation};

//expost HL sdk types
pub use hyperliquid_rust_sdk::{BaseUrl, Error};
pub use ethers::signers::LocalWallet;
