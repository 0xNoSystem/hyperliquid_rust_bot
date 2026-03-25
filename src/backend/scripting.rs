use rhai::{Engine, AST};

use crate::strategy::{
    BusyType, Intent, LiqSide, LimitOptions, OnTimeout, Order, ReduceOrder, SizeSpec, TimeoutInfo,
    Triggers,
};
use crate::{OpenPosInfo, Price, Side, TimeDelta, TimeFrame, Value, TimedValue};

// ── Compiled strategy (validated ASTs) ──────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CompiledStrategy {
    pub ast_on_idle: AST,
    pub ast_on_open: AST,
    pub ast_on_busy: AST,
}

// ── Engine factory ──────────────────────────────────────────────────────────

/// Create a Rhai `Engine` pre-configured with all trading domain types and
/// helper functions that strategy scripts can use.
pub fn create_engine() -> Engine {
    let mut engine = Engine::new();

    // Limit script execution to prevent abuse
    engine.set_max_operations(100_000);
    engine.set_max_expr_depths(64, 64);
    engine.set_max_string_size(4096);
    engine.set_max_array_size(1024);
    engine.set_max_map_size(256);

    register_side(&mut engine);
    register_price(&mut engine);
    register_open_pos_info(&mut engine);
    register_value(&mut engine);
    register_timed_value(&mut engine);
    register_size_spec(&mut engine);
    register_liq_side(&mut engine);
    register_triggers(&mut engine);
    register_intent(&mut engine);
    register_busy_type(&mut engine);
    register_timeframe(&mut engine);

    engine
}

/// Compile three strategy scripts (on_idle, on_open, on_busy) and return
/// compiled ASTs. Returns an error string describing which script(s) failed.
pub fn compile_strategy(
    engine: &Engine,
    on_idle: &str,
    on_open: &str,
    on_busy: &str,
) -> Result<CompiledStrategy, String> {
    let ast_on_idle = engine
        .compile(on_idle)
        .map_err(|e| format!("on_idle compile error: {}", e))?;
    let ast_on_open = engine
        .compile(on_open)
        .map_err(|e| format!("on_open compile error: {}", e))?;
    let ast_on_busy = engine
        .compile(on_busy)
        .map_err(|e| format!("on_busy compile error: {}", e))?;

    Ok(CompiledStrategy {
        ast_on_idle,
        ast_on_open,
        ast_on_busy,
    })
}

// ── Type registrations ──────────────────────────────────────────────────────

fn register_side(engine: &mut Engine) {
    engine.register_type_with_name::<Side>("Side");
    engine.register_fn("LONG", || Side::Long);
    engine.register_fn("SHORT", || Side::Short);
    engine.register_fn("==", |a: Side, b: Side| a == b);
    engine.register_fn("!=", |a: Side, b: Side| a != b);
    engine.register_fn("to_string", |s: &mut Side| match s {
        Side::Long => "Long".to_string(),
        Side::Short => "Short".to_string(),
    });
}

fn register_price(engine: &mut Engine) {
    engine.register_type_with_name::<Price>("Price");
    engine.register_get("open", |p: &mut Price| p.open);
    engine.register_get("high", |p: &mut Price| p.high);
    engine.register_get("low", |p: &mut Price| p.low);
    engine.register_get("close", |p: &mut Price| p.close);
    engine.register_get("vlm", |p: &mut Price| p.vlm);
    engine.register_get("open_time", |p: &mut Price| p.open_time as i64);
    engine.register_get("close_time", |p: &mut Price| p.close_time as i64);
}

fn register_open_pos_info(engine: &mut Engine) {
    engine.register_type_with_name::<OpenPosInfo>("OpenPosInfo");
    engine.register_get("side", |p: &mut OpenPosInfo| p.side);
    engine.register_get("size", |p: &mut OpenPosInfo| p.size);
    engine.register_get("entry_px", |p: &mut OpenPosInfo| p.entry_px);
    engine.register_get("open_time", |p: &mut OpenPosInfo| p.open_time as i64);
}

fn register_value(engine: &mut Engine) {
    engine.register_type_with_name::<Value>("Value");
    engine.register_fn("as_f64", |v: &mut Value| -> f64 {
        match *v {
            Value::RsiValue(x) => x,
            Value::EmaValue(x) => x,
            Value::SmaValue(x) => x,
            Value::SmaRsiValue(x) => x,
            Value::AdxValue(x) => x,
            Value::AtrValue(x) => x,
            Value::VolumeMaValue(x) => x,
            Value::StdDevValue(x) => x,
            Value::HistVolatilityValue(x) => x,
            Value::StochRsiValue { k, .. } => k,
            Value::EmaCrossValue { short, .. } => short,
        }
    });
    engine.register_fn("stoch_k", |v: &mut Value| -> f64 {
        match *v { Value::StochRsiValue { k, .. } => k, _ => f64::NAN }
    });
    engine.register_fn("stoch_d", |v: &mut Value| -> f64 {
        match *v { Value::StochRsiValue { d, .. } => d, _ => f64::NAN }
    });
    engine.register_fn("ema_short", |v: &mut Value| -> f64 {
        match *v { Value::EmaCrossValue { short, .. } => short, _ => f64::NAN }
    });
    engine.register_fn("ema_long", |v: &mut Value| -> f64 {
        match *v { Value::EmaCrossValue { long, .. } => long, _ => f64::NAN }
    });
    engine.register_fn("ema_trend", |v: &mut Value| -> bool {
        match *v { Value::EmaCrossValue { trend, .. } => trend, _ => false }
    });
}

fn register_timed_value(engine: &mut Engine) {
    engine.register_type_with_name::<TimedValue>("TimedValue");
    engine.register_get("value", |tv: &mut TimedValue| tv.value);
    engine.register_get("on_close", |tv: &mut TimedValue| tv.on_close);
    engine.register_get("ts", |tv: &mut TimedValue| tv.ts as i64);
}

fn register_size_spec(engine: &mut Engine) {
    engine.register_type_with_name::<SizeSpec>("SizeSpec");
    engine.register_fn("margin_amount", |amount: f64| SizeSpec::MarginAmount(amount));
    engine.register_fn("margin_pct", |pct: f64| SizeSpec::MarginPct(pct));
    engine.register_fn("raw_size", |sz: f64| SizeSpec::RawSize(sz));
}

fn register_liq_side(engine: &mut Engine) {
    engine.register_type_with_name::<LiqSide>("LiqSide");
    engine.register_type_with_name::<LimitOptions>("LimitOptions");
    engine.register_type_with_name::<TimeoutInfo>("TimeoutInfo");
    engine.register_type_with_name::<OnTimeout>("OnTimeout");
    engine.register_fn("TAKER", || LiqSide::Taker);
    engine.register_fn("FORCE", || OnTimeout::Force);
    engine.register_fn("CANCEL", || OnTimeout::Cancel);
}

fn register_triggers(engine: &mut Engine) {
    engine.register_type_with_name::<Triggers>("Triggers");
    engine.register_fn("triggers", |tp: f64, sl: f64| Triggers { tp: Some(tp), sl: Some(sl) });
    engine.register_fn("tp_only", |tp: f64| Triggers { tp: Some(tp), sl: None });
    engine.register_fn("sl_only", |sl: f64| Triggers { tp: None, sl: Some(sl) });
}

fn register_intent(engine: &mut Engine) {
    engine.register_type_with_name::<Intent>("Intent");
    engine.register_type_with_name::<Order>("Order");
    engine.register_type_with_name::<ReduceOrder>("ReduceOrder");

    engine.register_fn("open_market", |side: Side, size: SizeSpec| {
        Intent::open_market(side, size, None)
    });
    engine.register_fn("open_market", |side: Side, size: SizeSpec, trig: Triggers| {
        Intent::open_market(side, size, Some(trig))
    });
    engine.register_fn("flatten_market", || Intent::flatten_market());
    engine.register_fn("reduce_market", |size: SizeSpec| Intent::reduce_market_order(size));
    engine.register_fn("abort", || Intent::Abort);

    engine.register_fn("open_limit", |side: Side, size: SizeSpec, limit_px: f64| {
        Intent::open_limit(side, size, limit_px, None, None)
    });
    engine.register_fn("open_limit", |side: Side, size: SizeSpec, limit_px: f64, trig: Triggers| {
        Intent::open_limit(side, size, limit_px, None, Some(trig))
    });
    engine.register_fn("reduce_limit", |size: SizeSpec, limit_px: f64| {
        Intent::reduce_limit_order(size, limit_px, None)
    });
    engine.register_fn("flatten_limit", |limit_px: f64| Intent::flatten_limit(limit_px, None));

    engine.register_fn("arm", |td: TimeDelta| Intent::Arm(td));
    engine.register_fn("disarm", || Intent::Disarm);
}

fn register_busy_type(engine: &mut Engine) {
    engine.register_type_with_name::<BusyType>("BusyType");
    engine.register_fn("is_opening", |b: &mut BusyType| matches!(b, BusyType::Opening(_)));
    engine.register_fn("is_closing", |b: &mut BusyType| matches!(b, BusyType::Closing(_)));
}

fn register_timeframe(engine: &mut Engine) {
    engine.register_type_with_name::<TimeFrame>("TimeFrame");
    engine.register_type_with_name::<TimeDelta>("TimeDelta");
    engine.register_fn("MIN1", || TimeFrame::Min1);
    engine.register_fn("MIN3", || TimeFrame::Min3);
    engine.register_fn("MIN5", || TimeFrame::Min5);
    engine.register_fn("MIN15", || TimeFrame::Min15);
    engine.register_fn("MIN30", || TimeFrame::Min30);
    engine.register_fn("HOUR1", || TimeFrame::Hour1);
    engine.register_fn("HOUR2", || TimeFrame::Hour2);
    engine.register_fn("HOUR4", || TimeFrame::Hour4);
    engine.register_fn("HOUR12", || TimeFrame::Hour12);
    engine.register_fn("DAY1", || TimeFrame::Day1);
    engine.register_fn("timedelta", |tf: TimeFrame, count: i64| {
        TimeDelta::from_tf(tf, count as u64)
    });
}
