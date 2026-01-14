#![allow(unused_variables)]
#![allow(unused_assignments)]

use crate::signal::ValuesMap;
use crate::{IndexId, OpenPosInfo, Price, Side, TimeDelta, TimeFrame, timedelta};

const MARKET_ORDER_TIMEOUT: TimeDelta = timedelta!(TimeFrame::Min1, 1);

#[derive(Debug, Clone)]
pub struct StratContext<'a> {
    pub free_margin: f64,
    pub lev: usize,
    pub last_price: Price,
    pub indicators: &'a ValuesMap,
}

pub trait Strat: Send {
    fn on_idle(&mut self, ctx: StratContext, is_armed: Armed) -> Option<Intent>;
    fn on_busy(&mut self, ctx: StratContext, busy_reason: BusyType) -> Option<Intent>;
    fn on_open(&mut self, ctx: StratContext, open_pos: &OpenPosInfo) -> Option<Intent>;
    fn required_indicators(&self) -> Vec<IndexId>;
}

pub trait NeedsIndicators {
    fn required_indicators_static() -> Vec<IndexId>;
}

pub type Armed = Option<u64>; //expiry time

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct LiveTimeoutInfo {
    pub expire_at: u64,
    pub timeout_info: TimeoutInfo,
    pub intent: Intent,
}

impl LiveTimeoutInfo {
    pub fn expires_in(&self) -> TimeDelta {
        self.timeout_info.duration
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum BusyType {
    Opening(Option<LiveTimeoutInfo>),
    Closing(Option<LiveTimeoutInfo>),
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum SizeSpec {
    MarginAmount(f64),
    MarginPct(f64), // % of free margin OR % of open pos used_margin
    RawSize(f64),   // number of asset units
}

impl SizeSpec {
    pub(crate) fn get_size(&self, lev: f64, free_margin: f64, ref_px: f64) -> f64 {
        match self {
            SizeSpec::RawSize(sz) => *sz,
            SizeSpec::MarginAmount(amount) => (amount * lev) / ref_px,

            SizeSpec::MarginPct(pct) => {
                let amount = free_margin * (pct / 100.0);
                (amount * lev) / ref_px
            }
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum OnTimeout {
    Force,
    Cancel,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct TimeoutInfo {
    pub action: OnTimeout,
    pub duration: TimeDelta,
}

impl Default for TimeoutInfo {
    fn default() -> Self {
        TimeoutInfo {
            action: OnTimeout::Cancel,
            duration: MARKET_ORDER_TIMEOUT,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub enum LiqSide {
    Taker,
    Maker(LimitOptions), //limit_px hint
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct LimitOptions {
    pub limit_px: f64,
    pub timeout: Option<TimeoutInfo>,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Order {
    pub side: Side,
    pub size: SizeSpec,
    pub tp: Option<f64>,
    pub sl: Option<f64>,
    pub liq_side: LiqSide,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ReduceOrder {
    pub size: SizeSpec,
    pub liq_side: LiqSide,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum Intent {
    Open(Order),
    Reduce(ReduceOrder),
    Flatten(LiqSide),
    Arm(TimeDelta), //timeout duration
    Disarm,
    Abort, //Force close at market
}

#[derive(Copy, Clone, Debug)]
pub struct Triggers {
    pub tp: Option<f64>,
    pub sl: Option<f64>,
}

impl Intent {
    pub fn new_open(
        side: Side,
        size: SizeSpec,
        liq_side: LiqSide,
        tp_sl: Option<Triggers>,
    ) -> Self {
        let mut tp = None;
        let mut sl = None;
        if let Some(triggers) = tp_sl {
            tp = triggers.tp;
            sl = triggers.sl;
        }

        Intent::Open(Order {
            side,
            size,
            tp,
            sl,
            liq_side,
        })
    }

    pub fn open_market(side: Side, size: SizeSpec, tp_sl: Option<Triggers>) -> Self {
        Self::new_open(side, size, LiqSide::Taker, tp_sl)
    }

    pub fn open_limit(
        side: Side,
        size: SizeSpec,
        limit_px: f64,
        on_timeout: Option<TimeoutInfo>,
        tp_sl: Option<Triggers>,
    ) -> Self {
        let limit_options = LimitOptions {
            limit_px,
            timeout: on_timeout,
        };

        Self::new_open(side, size, LiqSide::Maker(limit_options), tp_sl)
    }

    pub fn reduce(size: SizeSpec, liq_side: LiqSide) -> Self {
        Intent::Reduce(ReduceOrder { size, liq_side })
    }

    pub fn reduce_market_order(size: SizeSpec) -> Self {
        Self::reduce(size, LiqSide::Taker)
    }

    pub fn reduce_limit_order(
        size: SizeSpec,
        limit_px: f64,
        on_timeout: Option<TimeoutInfo>,
    ) -> Self {
        let limit_options = LimitOptions {
            limit_px,
            timeout: on_timeout,
        };

        Self::reduce(size, LiqSide::Maker(limit_options))
    }

    pub fn flatten_market() -> Self {
        Intent::Flatten(LiqSide::Taker)
    }

    pub fn flatten_limit(limit_px: f64, on_timeout: Option<TimeoutInfo>) -> Self {
        let limit_options = LimitOptions {
            limit_px,
            timeout: on_timeout,
        };

        Intent::Flatten(LiqSide::Maker(limit_options))
    }
}

impl Intent {
    pub fn get_ttl(&self) -> Option<TimeoutInfo> {
        match self {
            Intent::Open(order) => match &order.liq_side {
                LiqSide::Maker(opts) => opts.timeout,
                LiqSide::Taker => None,
            },

            Intent::Reduce(order) => match &order.liq_side {
                LiqSide::Maker(opts) => opts.timeout,
                LiqSide::Taker => None,
            },

            Intent::Flatten(liq_side) => match liq_side {
                LiqSide::Maker(opts) => opts.timeout,
                LiqSide::Taker => None,
            },

            _ => None,
        }
    }

    pub fn is_order(&self) -> bool {
        matches!(
            self,
            Intent::Open(_) | Intent::Reduce(_) | Intent::Flatten(_)
        )
    }

    pub fn is_market_order(&self) -> bool {
        match self {
            Intent::Open(order) => match &order.liq_side {
                LiqSide::Maker(_) => false,
                LiqSide::Taker => true,
            },

            Intent::Reduce(order) => match &order.liq_side {
                LiqSide::Maker(_) => false,
                LiqSide::Taker => true,
            },

            Intent::Flatten(liq_side) => match liq_side {
                LiqSide::Maker(_) => false,
                LiqSide::Taker => true,
            },

            Intent::Abort => true,
            _ => false,
        }
    }

    pub fn is_limit_order(&self) -> bool {
        !self.is_market_order()
    }
}
