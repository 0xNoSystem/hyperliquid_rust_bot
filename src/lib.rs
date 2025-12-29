mod assets;
//mod backtest;
mod consts;
mod frontend;
mod helper;
mod market;
mod strategy;
mod strats;
mod trade_setup;
mod wallet;

pub mod bot;
mod exec;
pub mod margin;
pub mod signal;

pub use assets::MARKETS;
pub use strategy::*;
pub use strats::Strategy;
//pub use backtest::BackTester;
pub use bot::{Bot, BotEvent, BotToMarket};
pub use consts::*;
pub use exec::*;
pub use frontend::*;
pub use helper::*;
pub use margin::{AssetMargin, MarginAllocation};
pub use market::{AssetPrice, Market, MarketCommand, MarketUpdate};
pub use signal::{
    EditType, Entry, ExecParams, IndexId, IndicatorKind, OpenPosInfo, SignalEngine, TimedValue,
    ValuesMap,
};
pub use trade_setup::*;
pub use wallet::Wallet;

//exposed HL sdk types
pub use hyperliquid_rust_sdk::{AssetMeta, BaseUrl, Error, TradeInfo as HLTradeInfo};
pub use kwant::indicators::{Price, Value};
