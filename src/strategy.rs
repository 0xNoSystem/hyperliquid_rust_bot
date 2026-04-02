#![allow(unused_variables)]
#![allow(unused_assignments)]

use std::collections::HashMap;
use std::sync::Arc;

use log::warn;
use rhai::{Dynamic, Engine, Map, Scope};
use rustc_hash::FxHasher;
use std::hash::BuildHasherDefault;

use crate::backend::scripting::CompiledStrategy;
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

pub type Armed = Option<u64>; //expiry time

// ── Strategy (Rhai-powered Strat implementation) ────────────────────────────

/// Pre-computed mapping from `IndexId` → Rhai map key string.
/// Built once at construction, reused every tick to avoid per-tick `format!` allocations.
type IndicatorKeyMap = HashMap<IndexId, String, BuildHasherDefault<FxHasher>>;

fn build_indicator_keys(indicators: &[IndexId]) -> IndicatorKeyMap {
    indicators
        .iter()
        .map(|(kind, tf)| {
            let key = format!("{}_{}", kind.key(), tf.as_str());
            ((*kind, *tf), key)
        })
        .collect()
}

pub struct Strategy {
    engine: Arc<Engine>,
    compiled: CompiledStrategy,
    indicators: Vec<IndexId>,
    indicator_keys: IndicatorKeyMap,
    scope: Scope<'static>,
    /// Number of constants pushed at the start of scope; rewind target.
    scope_base: usize,
}

impl Strategy {
    pub fn new(engine: Arc<Engine>, compiled: CompiledStrategy, indicators: Vec<IndexId>) -> Self {
        let indicator_keys = build_indicator_keys(&indicators);
        let mut scope = Scope::new();
        push_scope_constants(&mut scope);
        scope.push("state", Map::new());
        let scope_base = scope.len();
        Self {
            engine,
            compiled,
            indicators,
            indicator_keys,
            scope,
            scope_base,
        }
    }

    pub fn reset_scope(&mut self) {
        self.scope = Scope::new();
        push_scope_constants(&mut self.scope);
        self.scope.push("state", Map::new());
    }

    /// Rewind scope to empty, then push fresh context variables.
    /// This avoids the per-variable linear name scan of `set_or_push`.
    fn push_context(&mut self, ctx: &StratContext) {
        self.scope.rewind(self.scope_base);
        self.scope.push("free_margin", ctx.free_margin);
        self.scope.push("lev", ctx.lev as i64);
        self.scope.push("last_price", ctx.last_price);
        self.scope
            .push("indicators", self.indicators_to_map(ctx.indicators));
    }

    /// Build the Rhai indicator map using pre-computed keys (no `format!` per tick).
    fn indicators_to_map(&self, values: &ValuesMap) -> Map {
        let mut map = Map::new();
        for ((kind, tf), timed_value) in values.iter() {
            if let Some(key) = self.indicator_keys.get(&(*kind, *tf)) {
                map.insert(key.as_str().into(), Dynamic::from(*timed_value));
            }
        }
        map
    }
}

/// Push trading-domain constants into a scope so scripts can reference them
/// as bare identifiers (e.g. `MIN15` instead of `MIN15()`).
fn push_scope_constants(scope: &mut Scope) {
    // Sides
    scope.push_constant("LONG", Side::Long);
    scope.push_constant("SHORT", Side::Short);

    // Timeframes
    scope.push_constant("MIN1", TimeFrame::Min1);
    scope.push_constant("MIN3", TimeFrame::Min3);
    scope.push_constant("MIN5", TimeFrame::Min5);
    scope.push_constant("MIN15", TimeFrame::Min15);
    scope.push_constant("MIN30", TimeFrame::Min30);
    scope.push_constant("HOUR1", TimeFrame::Hour1);
    scope.push_constant("HOUR2", TimeFrame::Hour2);
    scope.push_constant("HOUR4", TimeFrame::Hour4);
    scope.push_constant("HOUR12", TimeFrame::Hour12);
    scope.push_constant("DAY1", TimeFrame::Day1);

    // Liquidity / timeout actions
    scope.push_constant("TAKER", LiqSide::Taker);
    scope.push_constant("FORCE", OnTimeout::Force);
    scope.push_constant("CANCEL", OnTimeout::Cancel);
}

fn eval_ast(engine: &Engine, scope: &mut Scope, ast: &rhai::AST) -> Option<Intent> {
    match engine.eval_ast_with_scope::<Dynamic>(scope, ast) {
        Ok(result) => {
            if result.is_unit() {
                None
            } else {
                result.try_cast::<Intent>()
            }
        }
        Err(e) => {
            warn!("Rhai eval error: {}", e);
            None
        }
    }
}

impl Strat for Strategy {
    fn on_idle(&mut self, ctx: StratContext, is_armed: Armed) -> Option<Intent> {
        self.push_context(&ctx);
        self.scope
            .push("is_armed", is_armed.map(|t| t as i64).unwrap_or(-1_i64));
        eval_ast(&self.engine, &mut self.scope, &self.compiled.ast_on_idle)
    }

    fn on_open(&mut self, ctx: StratContext, open_pos: &OpenPosInfo) -> Option<Intent> {
        self.push_context(&ctx);
        self.scope.push("open_position", *open_pos);
        eval_ast(&self.engine, &mut self.scope, &self.compiled.ast_on_open)
    }

    fn on_busy(&mut self, ctx: StratContext, busy_reason: BusyType) -> Option<Intent> {
        self.push_context(&ctx);
        self.scope.push("busy_reason", busy_reason);
        eval_ast(&self.engine, &mut self.scope, &self.compiled.ast_on_busy)
    }

    fn required_indicators(&self) -> Vec<IndexId> {
        self.indicators.clone()
    }
}

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
