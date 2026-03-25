pub mod backend;

mod assets;
pub mod backtest;
pub mod broadcast;
mod consts;
mod frontend;
mod helper;
mod market;
mod strategy;
mod trade_setup;
mod wallet;

pub mod bot;
mod exec;
pub mod margin;
pub mod signal;

pub use assets::MARKETS;
pub use backtest::Backtester;
pub use bot::{Bot, BotEvent, BotToMarket};
pub use consts::*;
pub use exec::*;
pub use frontend::*;
pub use helper::*;
pub use margin::{AssetMargin, MarginAllocation};
pub use market::{AssetPrice, Market, MarketCommand, MarketState, MarketUpdate};
pub use signal::{
    BtAction, BtIntent, BtOrder, CloseOrder, EditType, EngineView, Entry, ExecParams, IndexId,
    OpenOrder, OpenPosInfo, SignalEngine, TimeFrameData, TimedValue, ValuesMap,
};
pub use strategy::*;
pub use trade_setup::*;
pub use wallet::Wallet;

//exposed HL sdk types
pub use hyperliquid_rust_sdk::{AssetMeta, BaseUrl, Error, TradeInfo as HLTradeInfo};
pub use kwant::indicators::{IndicatorKind, Price, Value};

use arraydeque::{ArrayDeque, behavior::Wrapping};
pub type CandleHistory = Box<ArrayDeque<Price, { MAX_HISTORY }, Wrapping>>;
