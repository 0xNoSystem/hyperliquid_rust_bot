#![allow(unused_variables)]
#![allow(unused_assignments)]

use std::collections::HashMap;
use std::sync::Arc;

use rhai::{Dynamic, Engine, Map, Scope};
use rustc_hash::FxHasher;
use std::hash::BuildHasherDefault;

use crate::backend::scripting::CompiledStrategy;
use crate::signal::ValuesMap;
use crate::{IndexId, IndicatorKind, OpenPosInfo, Price, Side, TimeDelta, TimeFrame, timedelta};

use tokio::sync::mpsc::Sender;
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

pub type Armed = Option<u64>;

// ── Strategy (Rhai-powered Strat implementation) ────────────────────────────

/// Pre-computed mapping from `IndexId` → Rhai map key string.
/// Built once at construction, reused every tick to avoid per-tick `format!` allocations.
type IndicatorKeyMap = HashMap<IndexId, String, BuildHasherDefault<FxHasher>>;

pub(crate) fn check_asset_fix(name: &str) -> String {
    name.replace(':', "_")
}

fn indicator_map_key(asset: &str, kind: IndicatorKind, tf: TimeFrame) -> String {
    format!("{}_{}_{}", check_asset_fix(asset), kind.key(), tf.as_str())
}

fn resolved_indicator_asset(asset: &Arc<str>, market_asset: &str) -> Arc<str> {
    if asset.as_ref() == "self" {
        Arc::from(market_asset)
    } else {
        Arc::clone(asset)
    }
}

pub(crate) fn replace_self_with_asset(market_asset: &str, indicators: &mut [IndexId]) {
    let market_asset: Arc<str> = Arc::from(market_asset);
    for (asset, _, _) in indicators.iter_mut() {
        if asset.as_ref() == "self" {
            *asset = Arc::clone(&market_asset);
        }
    }
}

fn build_indicator_keys(indicators: &[IndexId], market_asset: Arc<str>) -> IndicatorKeyMap {
    indicators
        .iter()
        .map(|(asset, kind, tf)| {
            let asset = resolved_indicator_asset(asset, market_asset.as_ref());
            let key = indicator_map_key(asset.as_ref(), *kind, *tf);
            ((Arc::clone(&asset), *kind, *tf), key)
        })
        .collect()
}

pub struct Strategy {
    engine: Arc<Engine>,
    log_tx: Option<Sender<String>>,
    compiled: CompiledStrategy,
    indicators: Vec<IndexId>,
    indicator_keys: IndicatorKeyMap,
    scope: Scope<'static>,
    scope_base: usize,
    asset: Arc<str>,
}

impl Strategy {
    pub fn new(
        engine: Arc<Engine>,
        compiled: CompiledStrategy,
        indicators: Vec<IndexId>,
        log_tx: Option<Sender<String>>,
        asset: Arc<str>,
    ) -> Self {
        let indicator_keys = build_indicator_keys(&indicators, asset.clone());
        let mut scope = Scope::new();
        push_scope_constants(&mut scope);
        scope.push("state", Map::new());
        let scope_base = scope.len();
        Self {
            engine,
            log_tx,
            compiled,
            indicators,
            indicator_keys,
            scope,
            scope_base,
            asset,
        }
    }

    pub fn reset_scope(&mut self) {
        self.scope = Scope::new();
        push_scope_constants(&mut self.scope);
        self.scope.push("state", Map::new());
    }

    fn sync_state_back(&mut self) {
        if self.compiled.state_var_names.is_empty() {
            return;
        }

        let values: Vec<(String, Dynamic)> = self
            .compiled
            .state_var_names
            .iter()
            .filter_map(|name| {
                self.scope
                    .get_value::<Dynamic>(name)
                    .map(|v| (name.clone(), v))
            })
            .collect();

        if let Some(state) = self.scope.get_value_mut::<Map>("state") {
            for (name, val) in values {
                state.insert(name.into(), val);
            }
        }
    }

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
        for ((asset, kind, tf), timed_value) in values.iter() {
            let resolved_asset = resolved_indicator_asset(asset, self.asset.as_ref());

            if let Some(key) = self
                .indicator_keys
                .get(&(Arc::clone(&resolved_asset), *kind, *tf))
            {
                map.insert(key.as_str().into(), Dynamic::from(*timed_value));
            }

            if resolved_asset.as_ref() == self.asset.as_ref() {
                let self_key = indicator_map_key("self", *kind, *tf);
                map.insert(self_key.into(), Dynamic::from(*timed_value));
            }
        }
        map
    }
}

fn push_scope_constants(scope: &mut Scope) {
    scope.push_constant("LONG", Side::Long);
    scope.push_constant("SHORT", Side::Short);

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

    scope.push_constant("TAKER", LiqSide::Taker);
    scope.push_constant("FORCE", OnTimeout::Force);
    scope.push_constant("CANCEL", OnTimeout::Cancel);
}

fn eval_ast(
    engine: &Engine,
    scope: &mut Scope,
    ast: &rhai::AST,
    log_tx: &Option<Sender<String>>,
) -> Option<Intent> {
    match engine.eval_ast_with_scope::<Dynamic>(scope, ast) {
        Ok(result) => {
            if result.is_unit() {
                None
            } else {
                result.try_cast::<Intent>()
            }
        }
        Err(e) => {
            if let Some(logger) = log_tx {
                let _ = logger.try_send(e.to_string());
            }
            None
        }
    }
}

impl Strat for Strategy {
    fn on_idle(&mut self, ctx: StratContext, is_armed: Armed) -> Option<Intent> {
        self.push_context(&ctx);
        self.scope
            .push("is_armed", is_armed.map(|t| t as i64).unwrap_or(-1_i64));
        let result = eval_ast(
            &self.engine,
            &mut self.scope,
            &self.compiled.ast_on_idle,
            &self.log_tx,
        );
        self.sync_state_back();
        result
    }

    fn on_open(&mut self, ctx: StratContext, open_pos: &OpenPosInfo) -> Option<Intent> {
        self.push_context(&ctx);
        self.scope.push("open_position", *open_pos);
        let result = eval_ast(
            &self.engine,
            &mut self.scope,
            &self.compiled.ast_on_open,
            &self.log_tx,
        );
        self.sync_state_back();
        result
    }

    fn on_busy(&mut self, ctx: StratContext, busy_reason: BusyType) -> Option<Intent> {
        self.push_context(&ctx);
        self.scope.push("busy_reason", busy_reason);
        let result = eval_ast(
            &self.engine,
            &mut self.scope,
            &self.compiled.ast_on_busy,
            &self.log_tx,
        );
        self.sync_state_back();
        result
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
    MarginPct(f64),
    RawSize(f64),
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::{Strategy, replace_self_with_asset};
    use crate::backend::scripting::{CompiledStrategy, create_engine};
    use crate::{IndicatorKind, TimeFrame, TimedValue, Value};

    #[test]
    fn replace_self_with_asset_normalizes_indicator_ids() {
        let mut indicators = vec![
            (
                Arc::<str>::from("self"),
                IndicatorKind::Rsi(14),
                TimeFrame::Min15,
            ),
            (
                Arc::<str>::from("SOL"),
                IndicatorKind::Ema(9),
                TimeFrame::Hour1,
            ),
        ];

        replace_self_with_asset("BTC", &mut indicators);

        assert_eq!(indicators[0].0.as_ref(), "BTC");
        assert_eq!(indicators[1].0.as_ref(), "SOL");
    }

    #[test]
    fn indicators_to_map_exposes_self_alias_for_market_asset_values() {
        let engine = Arc::new(create_engine());
        let compiled = CompiledStrategy::noop(engine.as_ref());
        let asset = Arc::<str>::from("BTC");
        let kind = IndicatorKind::Rsi(14);
        let tf = TimeFrame::Min15;

        let strategy = Strategy::new(
            engine,
            compiled,
            vec![(Arc::clone(&asset), kind, tf)],
            None,
            Arc::clone(&asset),
        );

        let mut values = crate::signal::ValuesMap::default();
        values.insert(
            (Arc::clone(&asset), kind, tf),
            TimedValue {
                value: Value::RsiValue(42.0),
                on_close: true,
                ts: 123,
            },
        );

        let map = strategy.indicators_to_map(&values);

        assert!(map.contains_key("BTC_rsi_14_15m"));
        assert!(map.contains_key("self_rsi_14_15m"));
    }
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
    Maker(LimitOptions),
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
    Arm(TimeDelta),
    Disarm,
    Abort,
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
            Intent::Open(order) => matches!(order.liq_side, LiqSide::Taker),
            Intent::Reduce(order) => matches!(order.liq_side, LiqSide::Taker),
            Intent::Flatten(liq_side) => matches!(liq_side, LiqSide::Taker),
            Intent::Abort => true,
            _ => false,
        }
    }

    pub fn is_limit_order(&self) -> bool {
        !self.is_market_order()
    }
}
