mod assets;
mod consts;
mod executor;
mod market;
mod wallet;
//mod backtest;

pub mod bot;
pub mod frontend;
pub mod helper;
pub mod margin;
pub mod signal;
pub mod strategy;
pub mod trade_setup;

pub use assets::MARKETS;
pub use bot::{Bot, BotEvent, BotToMarket};
pub use consts::MAX_HISTORY;
pub use executor::Executor;
pub use frontend::*;
pub use helper::*;
pub use market::{AssetPrice, Market, MarketCommand, MarketUpdate};
pub use signal::{EditType, Entry, IndexId, IndicatorKind, SignalEngine};
pub use wallet::Wallet;
// pub use backtest::BackTester;
pub use margin::{AssetMargin, MarginAllocation};
pub use trade_setup::{
    LiquidationFillInfo, MarketTradeInfo, TimeFrame, TradeCommand, TradeFillInfo, TradeInfo,
    TradeParams,
};

//expost HL sdk types
pub use ethers::signers::LocalWallet;
pub use hyperliquid_rust_sdk::{BaseUrl, Error};
pub use kwant::indicators::Value;
