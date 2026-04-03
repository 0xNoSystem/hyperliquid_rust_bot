use std::collections::HashMap;

use regex::Regex;
use rhai::{AST, Dynamic, Engine, Scope};

use crate::strategy::{
    BusyType, Intent, LimitOptions, LiqSide, OnTimeout, Order, ReduceOrder, SizeSpec, TimeoutInfo,
    Triggers,
};
use crate::{OpenPosInfo, Price, Side, TimeDelta, TimeFrame, TimedValue, Value};

/// State variable declarations: variable name → default value as Rhai literal.
pub type StateDeclarations = HashMap<String, serde_json::Value>;

// ── Compiled strategy (validated ASTs) ──────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CompiledStrategy {
    pub ast_on_idle: AST,
    pub ast_on_open: AST,
    pub ast_on_busy: AST,
    /// Names of user-declared state variables (for post-eval sync-back).
    pub state_var_names: Vec<String>,
}

impl CompiledStrategy {
    /// A no-op strategy that never emits any trading signals.
    pub fn noop(engine: &Engine) -> Self {
        let ast = engine.compile("()").expect("noop script must compile");
        Self {
            ast_on_idle: ast.clone(),
            ast_on_open: ast.clone(),
            ast_on_busy: ast,
            state_var_names: Vec::new(),
        }
    }
}

// ── Engine factory ──────────────────────────────────────────────────────────

/// Create a Rhai `Engine` pre-configured with all trading domain types and
/// helper functions that strategy scripts can use.
pub fn create_engine() -> Engine {
    let mut engine = Engine::new();
    engine.set_optimization_level(rhai::OptimizationLevel::Full);
    engine.set_strict_variables(true);

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

/// Build a Rhai scope declaring all variables a strategy script may reference.
/// The values are dummies — only the *names* matter for strict-variable checking.
fn validation_scope(extra: &[&str]) -> Scope<'static> {
    let mut scope = Scope::new();

    // Constants (sides + timeframes + liquidity + timeout actions)
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

    // Context variables present in every script
    scope.push("free_margin", 0.0_f64);
    scope.push("lev", 0_i64);
    scope.push("last_price", Dynamic::UNIT);
    scope.push("indicators", Dynamic::UNIT);
    scope.push("state", Dynamic::UNIT);

    // Script-specific variables
    for name in extra {
        scope.push(*name, Dynamic::UNIT);
    }
    scope
}

/// Compile three strategy scripts (on_idle, on_open, on_busy) and return
/// compiled ASTs. With strict variables enabled, the compiler rejects any
/// reference to an undefined variable across ALL code branches.
///
/// Raw scripts are expanded (extract macros + state init preamble) before
/// compilation. The DB stores the raw user code; expansion is transient.
pub fn compile_strategy(
    engine: &Engine,
    on_idle: &str,
    on_open: &str,
    on_busy: &str,
    state_declarations: Option<&StateDeclarations>,
) -> Result<CompiledStrategy, String> {
    let state_preamble = state_declarations
        .map(generate_state_preamble)
        .unwrap_or_default();

    let expanded_idle = expand_script(on_idle, &state_preamble);
    let expanded_open = expand_script(on_open, &state_preamble);
    let expanded_busy = expand_script(on_busy, &state_preamble);

    // State variable names need to be in scope for strict-variable checking
    let state_var_names: Vec<String> = state_declarations
        .map(|d| d.keys().cloned().collect())
        .unwrap_or_default();
    let state_var_refs: Vec<&str> = state_var_names.iter().map(|s| s.as_str()).collect();

    let mut idle_extras = vec!["is_armed"];
    idle_extras.extend_from_slice(&state_var_refs);
    let mut open_extras = vec!["open_position"];
    open_extras.extend_from_slice(&state_var_refs);
    let mut busy_extras = vec!["busy_reason"];
    busy_extras.extend_from_slice(&state_var_refs);

    let idle_scope = validation_scope(&idle_extras);
    let open_scope = validation_scope(&open_extras);
    let busy_scope = validation_scope(&busy_extras);

    let ast_on_idle = engine
        .compile_with_scope(&idle_scope, &expanded_idle)
        .map_err(|e| format!("on_idle compile error: {}", e))?;
    let ast_on_open = engine
        .compile_with_scope(&open_scope, &expanded_open)
        .map_err(|e| format!("on_open compile error: {}", e))?;
    let ast_on_busy = engine
        .compile_with_scope(&busy_scope, &expanded_busy)
        .map_err(|e| format!("on_busy compile error: {}", e))?;

    Ok(CompiledStrategy {
        ast_on_idle,
        ast_on_open,
        ast_on_busy,
        state_var_names,
    })
}

// ── Script expansion (transpilation) ───────────────────────────────────────

/// Expand a raw user script: apply extract() macro expansion, then prepend
/// the state initialization preamble.
fn expand_script(src: &str, state_preamble: &str) -> String {
    let expanded = expand_extract(src);
    if state_preamble.is_empty() {
        expanded
    } else {
        format!("{}\n{}", state_preamble, expanded)
    }
}

/// Expand `let <var> = extract("<key>");` into indicator access + guard +
/// value unpacking. The unpacking depends on the indicator type detected
/// from the key prefix.
fn expand_extract(src: &str) -> String {
    let re = Regex::new(r#"let\s+(\w+)\s*=\s*extract\(\s*"([^"]+)"\s*\)\s*;"#).unwrap();

    re.replace_all(src, |caps: &regex::Captures| {
        let var = &caps[1];
        let key = &caps[2];

        let mut out = format!("let {var} = indicators[\"{key}\"];\nif {var} == () {{ return; }}\n");

        if key.starts_with("stochRsi_") {
            out.push_str(&format!(
                "let {var}_k = {var}.value.stoch_k();\n\
                 let {var}_d = {var}.value.stoch_d();\n\
                 let {var}_on_close = {var}.on_close;\n\
                 let {var}_ts = {var}.ts;\n"
            ));
        } else if key.starts_with("emaCross_") {
            out.push_str(&format!(
                "let {var}_short = {var}.value.ema_short();\n\
                 let {var}_long = {var}.value.ema_long();\n\
                 let {var}_trend = {var}.value.ema_trend();\n\
                 let {var}_on_close = {var}.on_close;\n\
                 let {var}_ts = {var}.ts;\n"
            ));
        } else {
            out.push_str(&format!(
                "let {var}_value = {var}.value.as_f64();\n\
                 let {var}_on_close = {var}.on_close;\n\
                 let {var}_ts = {var}.ts;\n"
            ));
        }
        out
    })
    .into_owned()
}

/// Generate the state initialization preamble from state declarations.
/// Each declaration becomes: `let <name> = if state["<name>"] == () { <default> } else { state["<name>"] };`
fn generate_state_preamble(decls: &StateDeclarations) -> String {
    let mut lines = Vec::with_capacity(decls.len());
    for (name, default) in decls {
        let default_rhai = json_to_rhai_literal(default);
        lines.push(format!(
            "let {name} = if state[\"{name}\"] == () {{ {default_rhai} }} else {{ state[\"{name}\"] }};"
        ));
    }
    lines.join("\n")
}

/// Convert a serde_json::Value to a Rhai literal string.
fn json_to_rhai_literal(val: &serde_json::Value) -> String {
    match val {
        serde_json::Value::Null => "()".to_string(),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else if let Some(f) = n.as_f64() {
                let s = f.to_string();
                if s.contains('.') { s } else { format!("{s}.0") }
            } else {
                "0".to_string()
            }
        }
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::String(s) => {
            format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
        }
        _ => "()".to_string(),
    }
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
        match *v {
            Value::StochRsiValue { k, .. } => k,
            _ => f64::NAN,
        }
    });
    engine.register_fn("stoch_d", |v: &mut Value| -> f64 {
        match *v {
            Value::StochRsiValue { d, .. } => d,
            _ => f64::NAN,
        }
    });
    engine.register_fn("ema_short", |v: &mut Value| -> f64 {
        match *v {
            Value::EmaCrossValue { short, .. } => short,
            _ => f64::NAN,
        }
    });
    engine.register_fn("ema_long", |v: &mut Value| -> f64 {
        match *v {
            Value::EmaCrossValue { long, .. } => long,
            _ => f64::NAN,
        }
    });
    engine.register_fn("ema_trend", |v: &mut Value| -> bool {
        match *v {
            Value::EmaCrossValue { trend, .. } => trend,
            _ => false,
        }
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
    engine.register_fn("margin_amount", |amount: f64| {
        SizeSpec::MarginAmount(amount)
    });
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
    engine.register_fn("timeout", |action: OnTimeout, duration: TimeDelta| {
        TimeoutInfo { action, duration }
    });
}

fn register_triggers(engine: &mut Engine) {
    engine.register_type_with_name::<Triggers>("Triggers");
    engine.register_fn("triggers", |tp: f64, sl: f64| Triggers {
        tp: Some(tp),
        sl: Some(sl),
    });
    engine.register_fn("tp_only", |tp: f64| Triggers {
        tp: Some(tp),
        sl: None,
    });
    engine.register_fn("sl_only", |sl: f64| Triggers {
        tp: None,
        sl: Some(sl),
    });
}

fn register_intent(engine: &mut Engine) {
    engine.register_type_with_name::<Intent>("Intent");
    engine.register_type_with_name::<Order>("Order");
    engine.register_type_with_name::<ReduceOrder>("ReduceOrder");

    engine.register_fn("open_market", |side: Side, size: SizeSpec| {
        Intent::open_market(side, size, None)
    });
    engine.register_fn(
        "open_market",
        |side: Side, size: SizeSpec, trig: Triggers| Intent::open_market(side, size, Some(trig)),
    );
    engine.register_fn("flatten_market", Intent::flatten_market);
    engine.register_fn("reduce_market", |size: SizeSpec| {
        Intent::reduce_market_order(size)
    });
    engine.register_fn("abort", || Intent::Abort);

    engine.register_fn("open_limit", |side: Side, size: SizeSpec, limit_px: f64| {
        Intent::open_limit(side, size, limit_px, None, None)
    });
    engine.register_fn(
        "open_limit",
        |side: Side, size: SizeSpec, limit_px: f64, trig: Triggers| {
            Intent::open_limit(side, size, limit_px, None, Some(trig))
        },
    );
    engine.register_fn(
        "open_limit",
        |side: Side, size: SizeSpec, limit_px: f64, ttl: TimeoutInfo| {
            Intent::open_limit(side, size, limit_px, Some(ttl), None)
        },
    );
    engine.register_fn(
        "open_limit",
        |side: Side, size: SizeSpec, limit_px: f64, ttl: TimeoutInfo, trig: Triggers| {
            Intent::open_limit(side, size, limit_px, Some(ttl), Some(trig))
        },
    );
    engine.register_fn("reduce_limit", |size: SizeSpec, limit_px: f64| {
        Intent::reduce_limit_order(size, limit_px, None)
    });
    engine.register_fn(
        "reduce_limit",
        |size: SizeSpec, limit_px: f64, ttl: TimeoutInfo| {
            Intent::reduce_limit_order(size, limit_px, Some(ttl))
        },
    );
    engine.register_fn("flatten_limit", |limit_px: f64| {
        Intent::flatten_limit(limit_px, None)
    });
    engine.register_fn("flatten_limit", |limit_px: f64, ttl: TimeoutInfo| {
        Intent::flatten_limit(limit_px, Some(ttl))
    });

    engine.register_fn("arm", |td: TimeDelta| Intent::Arm(td));
    engine.register_fn("disarm", || Intent::Disarm);
}

fn register_busy_type(engine: &mut Engine) {
    engine.register_type_with_name::<BusyType>("BusyType");
    engine.register_fn("is_opening", |b: &mut BusyType| {
        matches!(b, BusyType::Opening(_))
    });
    engine.register_fn("is_closing", |b: &mut BusyType| {
        matches!(b, BusyType::Closing(_))
    });
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
