mod assets;
//mod backtest;
mod consts;
mod frontend;
mod helper;
mod market;
mod trade_setup;
mod wallet;

pub mod bot;
mod exec;
pub mod margin;
pub mod signal;
pub mod strategy;

pub use assets::MARKETS;
//pub use backtest::BackTester;
pub use bot::{Bot, BotEvent, BotToMarket};
pub use consts::MAX_HISTORY;
pub use exec::*;
pub use frontend::*;
pub use helper::*;
pub use margin::{AssetMargin, MarginAllocation};
pub use market::{AssetPrice, Market, MarketCommand, MarketUpdate};
pub use signal::{EditType, Entry, IndexId, IndicatorKind, SignalEngine};
pub use trade_setup::*;
pub use wallet::Wallet;

//expost HL sdk types
pub use hyperliquid_rust_sdk::{BaseUrl, Error, TradeInfo as HLTradeInfo};
pub use kwant::indicators::{Price, Value};
