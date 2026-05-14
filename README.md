# Hyperliquid Trading Terminal

A multi-user perpetual futures trading terminal for [Hyperliquid](https://hyperliquid.xyz), featuring a scripting engine that lets you write, test, and deploy automated trading strategies from your browser.

Built with a Rust backend (Axum + WebSocket) and a React frontend. Strategies are written in **Rhai** (a Rust-native scripting language), execute server-side with sandboxed resource limits, and can combine indicator signals across multiple assets and timeframes.

---

## How It Works

```
Browser (React)                     Server (Rust)
 +-----------+    REST / WS    +-------------------+
 | Strategy  | -------------> | Bot per user       |
 |  Editor   |                | SignalEngine        |
 | Backtest  |                |  -> Rhai scripts    |
 | Dashboard |  <------------ |  -> Indicators      |
 +-----------+   live state   |  -> Executor -> HL  |
                              +-------------------+
```

1. You connect your Hyperliquid wallet (Ethereum signature auth).
2. You add markets, choose leverage, and allocate margin.
3. You configure indicators per asset/timeframe and write a strategy using the scripting API (or use an existing one).
4. The engine keeps those feeds hot, evaluates your scripts on each relevant candle update, and executes the resulting orders on Hyperliquid.

---

## Production Operations

Required backend env:

```bash
DATABASE_URL=postgres://...
JWT_SECRET=at_least_32_random_bytes
ENCRYPTION_KEY=64_hex_chars
```

Recommended production env:

```bash
SERVER_BIND_ADDR=0.0.0.0:8090
DATABASE_MAX_CONNECTIONS=10
DATABASE_CONNECT_TIMEOUT_SECONDS=10
DATABASE_ACQUIRE_TIMEOUT_SECONDS=5
CORS_ORIGINS=https://your-ui.example
QUICKNODE_HYPERCORE_ENDPOINTS=https://endpoint-1...,https://endpoint-2...
# Or configure one build-account endpoint per variable:
QUICKNODE_HYPERCORE_ENDPOINT1=https://endpoint-1...
QUICKNODE_HYPERCORE_ENDPOINT2=https://endpoint-2...
# ...
QUICKNODE_HYPERCORE_ENDPOINT10=https://endpoint-10...
```

QuickNode account-event streaming is used when a QuickNode HyperCore endpoint is configured. Before promoting a deployment, run the gated soak against the same provider account and at least two representative user addresses:

```bash
QN_SOAK_USERS='0xabc...,0xdef...' \
QN_SOAK_SECONDS=900 \
QN_SOAK_RECONNECT_EVERY_SECONDS=120 \
QN_SOAK_CHURN_EVERY_SECONDS=60 \
cargo run --release --bin qn_soak
```

The soak exits non-zero if QuickNode returns JSON-RPC errors, subscription acknowledgements are missing, or configured reconnect/churn cycles do not execute. For the final live gate, run it against a wallet that will intentionally produce an account event during the window, then set `QN_SOAK_REQUIRE_EVENTS=true` or `QN_SOAK_REQUIRE_ACCOUNT_EVENTS=true` to require at least one routed payload before passing.

Health checks before deploy:

```bash
curl -fsS http://127.0.0.1:8090/healthz
curl -fsS http://127.0.0.1:8090/readyz
./ci.sh
cargo run --release --bin load -- --bots 100 --markets-per-bot 3 --ticks 2000 --account-events 1000 --queue 128 --slow-every 25 --slow-delay-us 250
```

Runtime overload counters are exposed from the authenticated `GET /metrics` endpoint. Watch the `*Dropped` and `*Lagged` counters during load, reconnects, and frontend fanout.

`CORS_ORIGINS` is fail-closed when unset or invalid. Use a comma-separated list of browser origins in production, or set `CORS_ORIGINS=*` only for local development.

---

## LLM Strategy Generation Prompt

Copy this prompt into an LLM chat before asking it to generate a KWANT strategy. For best results, paste this prompt first, then paste this README or the `/docs` page content after it as the reference material.

```text
You are a strategy-generation assistant for KWANT, a Hyperliquid trading terminal where strategies are written as three Rhai scripts: on_idle, on_open, and on_busy.

The KWANT strategy API reference follows this prompt. Treat that reference as the source of truth. Use only variables, helpers, indicators, constants, object fields, and behavior documented there. Do not invent helper functions, indicators, order types, fields, imports, classes, async code, or external libraries.

When the user asks for a strategy:

1. If required details are missing, ask up to five concise clarifying questions. Required details usually include traded market, directional bias if any, timeframe, indicators/signals, risk sizing, exit behavior, and whether limit-order TTLs should FORCE or CANCEL. If the user asks you to decide, choose conservative defaults and state the assumptions.
2. Return a complete strategy package with these headings:
   - Strategy name
   - Intended market
   - Indicator configuration: list each asset, indicator, parameters, timeframe, and exact extract key
   - State declarations: a plain block suitable for the State Variables editor
   - on_idle: Rhai code block
   - on_open: Rhai code block
   - on_busy: Rhai code block
   - Behavior notes
   - Backtest checklist
3. Rhai code must compile under strict variables. Every referenced variable must be provided by the engine, declared as state, declared locally, or generated by an extract() statement.
4. Use extract() only in the supported source-transform shape:
   let name = extract("KEY");
   The key must be a string literal. After extraction, use generated names such as name_value, name_on_close, name_ts, name_short, name_long, name_trend, name_k, name_d, name_macd, name_signal, name_histogram, name_upper, name_mid, name_lower, or name_width according to indicator type.
5. Prefer self_ indicator keys for the traded market so the strategy is reusable across assets. Use explicit asset-prefixed keys for cross-asset filters.
6. Avoid repeated trades on the same candle. For candle-close logic, check name_on_close and usually store name_ts in state, then skip if it has already been processed.
7. In cross-asset strategies, do not assume last_price belongs to the traded market. Avoid deriving traded-market limit prices from last_price.close unless the strategy is intentionally single-asset or guarded by a self_ timestamp.
8. Use only documented order helpers. Market-vs-limit is chosen by helper name, not by a MAKER constant. FORCE TTL means cancel the target limit order, then submit the market equivalent. CANCEL TTL means cancel resting orders and defensively force-close if exposure exists.
9. If requested behavior cannot be represented with the documented API, say so clearly and provide the closest valid alternative.
10. Do not present generated strategies as financial advice or as profitable. Tell the user to backtest and inspect orders before live deployment.

Code style:
- Keep code blocks pure Rhai with no Markdown inside them.
- Use explicit returns when exiting early.
- Keep on_busy empty unless abort behavior is intentionally required.
- Use state names that are valid identifiers and do not collide with engine variables or constants.
```

---

## Strategy Scripting API

A strategy is three scripts that run at different stages of a trade lifecycle:

| Script | When it runs | Extra variable |
|--------|-------------|----------------|
| **`on_idle`** | No position is open | `is_armed` -- expiry timestamp (or `-1` if not armed) |
| **`on_open`** | A position is open | `open_position` -- current position info |
| **`on_busy`** | An order is pending (waiting for fill) | `busy_reason` -- opening or closing info |

Each script can return an **Intent** (a trading action) or return nothing (do nothing).

### Authoring Rules

The backend saves your raw Rhai code in the database, but validates a transient expanded version first. Expansion currently does two things:

- Rewrites supported `extract()` calls into real indicator-map access.
- Prepends state-variable initialization generated from the State Variables box.

The Rhai compiler runs with **strict variables enabled**, so any variable referenced anywhere in a script must be declared, provided by the engine, or generated by `extract()`/state expansion. Typos are rejected when saving the strategy, before the bot runs.

Return an intent as the script's final expression, or use `return` explicitly:

```rust
// Both forms are valid
open_market(LONG, margin_pct(50.0))

return open_market(LONG, margin_pct(50.0));
```

A trailing semicolon makes an expression evaluate to `()`, which means "do nothing" unless you used `return`.

```rust
open_market(LONG, margin_pct(50.0));  // no intent is returned
```

Use `print("message")` for debugging. In live mode, printed messages and runtime errors are sent to the market log. Runtime errors do not crash the bot; that tick simply returns no intent.

### Market Model

A strategy is attached to one market. Order helpers such as `open_market`, `open_limit`, `flatten_*`, and `reduce_*` act on that market.

Indicators are separate inputs. Each configured indicator is identified by **asset + indicator kind + timeframe** (internally this is the runtime `IndexId`), so one script can read BTC, SOL, ETH, and other signals side by side.

The traded market also gets a `self_...` indicator alias. If a strategy is attached to SOL, these two keys refer to the same configured SOL RSI indicator:

```rust
let sol_rsi = extract("SOL_rsi_14_15m");
let same_rsi = extract("self_rsi_14_15m");
```

`last_price` is the candle that triggered the current evaluation tick. In single-asset strategies that is normally the traded market candle. In cross-asset strategies, it can be a candle from another configured asset, and the current tick asset is not exposed directly to Rhai. Prefer market orders or stateful `self_..._ts` guards when reacting to market-asset candles.

### Context Variables

These are available in **all three** scripts:

| Variable | Type | Description |
|----------|------|-------------|
| `free_margin` | `f64` | Available margin in USDC |
| `lev` | `i64` | Current leverage multiplier |
| `last_price` | `Price` | Latest candle data for the current evaluation tick |
| `indicators` | `Map` | Configured indicator values across all strategy assets/timeframes (use `extract()` instead of accessing directly) |

State variables declared in the editor are also available in all scripts as bare variables. See [State Declarations](#state-declarations).

### Constants

**Sides**
```
LONG    SHORT
```

**Timeframes**
```
MIN1    MIN3    MIN5    MIN15   MIN30
HOUR1   HOUR2   HOUR4   HOUR12  DAY1
```

These constants are available to `timedelta()`. Indicator key suffixes are the UI timeframe strings: `1m`, `3m`, `5m`, `15m`, `30m`, `1h`, `2h`, `4h`, `12h`, `1d`, `3d`, `1w`, `1M`.

**Market vs limit orders**

There is no `MAKER` constant in the scripting API. Choose order liquidity through the helper you return:

```rust
open_market(...)     // taker-style market execution
open_limit(...)      // maker-style limit order
flatten_market()     // market close
flatten_limit(px)    // limit close
reduce_market(...)   // market partial close
reduce_limit(...)    // limit partial close
```

`TAKER` exists internally as a registered constant, but normal strategies should not need it.

**Timeout actions**
```
FORCE           -- cancel the target limit order, then submit the market equivalent
CANCEL          -- cancel resting orders and defensively force-close if needed
```

---

## Intents (Trading Actions)

Scripts return an intent to tell the engine what to do. Return nothing (no explicit return, or `()`) to skip the tick.

### Opening Positions

```rust
// Market orders
open_market(LONG, margin_pct(50.0))
open_market(SHORT, margin_amount(100.0))
open_market(LONG, margin_pct(100.0), triggers(5.0, 3.0))

// Limit orders
open_limit(LONG, margin_pct(50.0), 42000.0)
open_limit(SHORT, raw_size(0.5), 42000.0, triggers(5.0, 3.0))
open_limit(LONG, margin_pct(50.0), 42000.0, timeout(FORCE, timedelta(MIN15, 2)))
open_limit(LONG, margin_pct(50.0), 42000.0, timeout(CANCEL, timedelta(HOUR1, 1)), triggers(5.0, 3.0))
```

### Closing Positions

```rust
// Close entire position at market
flatten_market()

// Close entire position with a limit order
flatten_limit(43000.0)
flatten_limit(43000.0, timeout(FORCE, timedelta(MIN5, 3)))

// Partial close
reduce_market(margin_pct(50.0))
reduce_limit(margin_pct(50.0), 43000.0)
reduce_limit(raw_size(0.1), 43000.0, timeout(CANCEL, timedelta(MIN15, 1)))
```

### Other Actions

```rust
// Abort: force close everything at market immediately
abort()

// Arm: delay entry -- on_idle will receive is_armed != -1 until expiry
arm(timedelta(MIN5, 3))

// Disarm: cancel the armed state
disarm()
```

### Runtime Intent Rules

- `on_idle` should open, arm, or disarm. Opening is only accepted when the engine is idle.
- `on_open` should flatten or reduce the current position.
- `on_busy` runs while an open/close order is pending. Any returned intent is ignored while busy except `abort()`.
- `arm(timedelta(...))` only works from idle state. While armed, `on_idle` receives `is_armed` as the expiry timestamp in milliseconds; otherwise it receives `-1`.
- `disarm()` only has an effect while armed.
- `abort()` forces a market close, clears pending trigger state, and moves the engine back to idle.
- Market orders get an internal one-minute pending timeout. Limit orders can define their own timeout with `timeout(FORCE, ...)` or `timeout(CANCEL, ...)`.
- `timeout(FORCE, duration)` cancels the target non-TP/SL limit order first, then submits the market equivalent of the original open/reduce order. `flatten_limit(..., timeout(FORCE, ...))` cancels tracked resting orders first, then force-closes at market.
- `timeout(CANCEL, duration)` cancels tracked resting orders. If a position exists after cancellation, the executor force-closes it at market. For an unfilled open limit, this behaves like a plain cancel; for a partially filled open or pending close, it is defensive and exits exposure.

Orders are validated before they are sent:

- Open order notional must be at least `$10`.
- Partial close notional must be at least `$10`; a full close may be smaller.
- Open size cannot exceed `free_margin * lev / reference_price`.
- Limit prices must be positive and between `5%` and `1500%` of the current reference price.
- TP must be positive. SL must be positive and less than `100`.

---

## Size Specification

| Function | Description |
|----------|-------------|
| `margin_pct(pct)` | Percentage of free margin (e.g. `margin_pct(100.0)` = all-in) |
| `margin_amount(usdc)` | Fixed USDC amount as margin |
| `raw_size(units)` | Exact number of asset units |

Size is converted to asset units at execution time using: `(margin * leverage) / reference_price`.

---

## Triggers (TP/SL)

Triggers are specified as **percentage** values relative to entry price, adjusted for leverage.

```rust
triggers(5.0, 3.0)   // 5% TP, 3% SL
tp_only(5.0)          // TP only
sl_only(3.0)          // SL only
```

- TP must be positive
- SL must be positive and less than 100

For a long position, `triggers(5.0, 3.0)` with `10x` leverage places TP at `entry * (1 + 0.05 / 10)` and SL at `entry * (1 - 0.03 / 10)`. Shorts invert the direction.

---

## Indicators

Add indicators to your strategy through the editor UI. Each indicator is now bound to an **asset** and a timeframe, then exposed to Rhai through the `indicators` map.

### Asset-Specific Keys

Indicators are asset-scoped in scripts. For example:

```rust
let sol_rsi = extract("SOL_rsi_12_1h");
let btc_rsi = extract("BTC_rsi_12_1h");
let market_rsi = extract("self_rsi_12_1h");
```

This lets a strategy attached to one market use confirmation signals from other assets without duplicating the scripting model.

Keys must match an indicator configured for the strategy. Asset symbols containing `:` are normalized to `_` during lookup, so an asset displayed as `PURR/USDC:USDC` would use `PURR/USDC_USDC_...` in the key. The editor's indicator badges insert the exact `extract()` call and are the safest source of truth.

### Key Timeframe Suffixes

Use these suffixes inside indicator keys:

| UI timeframe | Key suffix |
|--------------|------------|
| 1 minute | `1m` |
| 3 minutes | `3m` |
| 5 minutes | `5m` |
| 15 minutes | `15m` |
| 30 minutes | `30m` |
| 1 hour | `1h` |
| 2 hours | `2h` |
| 4 hours | `4h` |
| 12 hours | `12h` |
| 1 day | `1d` |
| 3 days | `3d` |
| 1 week | `1w` |
| 1 month | `1M` |

### Available Indicators

The terminal now supports **19 indicators total**, including **9 recently added** ones: DEMA, TEMA, OBV, VWAP Deviation, CCI, Ichimoku, MACD, ROC, and Bollinger Bands.

| Indicator | Key format | Parameters |
|-----------|-----------|------------|
| RSI | `{asset}_rsi_{periods}_{tf}` | periods |
| EMA | `{asset}_ema_{periods}_{tf}` | periods |
| DEMA | `{asset}_dema_{periods}_{tf}` | periods |
| TEMA | `{asset}_tema_{periods}_{tf}` | periods |
| SMA | `{asset}_sma_{periods}_{tf}` | periods |
| ATR | `{asset}_atr_{periods}_{tf}` | periods |
| ADX | `{asset}_adx_{periods}_{di_length}_{tf}` | periods, DI length |
| Stochastic RSI | `{asset}_stochRsi_{periods}_{k}_{d}_{tf}` | periods, K smoothing, D smoothing |
| SMA on RSI | `{asset}_smaRsi_{periods}_{smoothing}_{tf}` | periods, smoothing length |
| EMA Cross | `{asset}_emaCross_{short}_{long}_{tf}` | short period, long period |
| MACD | `{asset}_macd_{fast}_{slow}_{signal}_{tf}` | fast, slow, signal |
| Ichimoku | `{asset}_ichimoku_{tenkan}_{kijun}_{senkou_b}_{tf}` | tenkan, kijun, senkou_b |
| Bollinger Bands | `{asset}_bollinger_{periods}_{std}_{tf}` | periods, std multiplier (UI value is x100, e.g. 200 -> key uses `2`) |
| ROC | `{asset}_roc_{periods}_{tf}` | periods |
| OBV | `{asset}_obv_{tf}` | none |
| Volume MA | `{asset}_volMa_{periods}_{tf}` | periods |
| Historical Volatility | `{asset}_histVol_{periods}_{tf}` | periods |
| VWAP Deviation | `{asset}_vwapDeviation_{periods}_{tf}` | periods |
| CCI | `{asset}_cci_{periods}_{tf}` | periods |

Bollinger Bands use the UI's `std_multiplier_x100` divided by 100 in the key: `200` becomes `2`, `250` becomes `2.5`, and `225` becomes `2.25`.

### The `extract()` Macro

Use `extract()` to access an indicator. It handles the lookup, null guard, and value unpacking automatically. You write one line and get several ready-to-use variables.

`extract()` is a source transform, not a normal Rhai function. It must be written in this exact statement shape:

```rust
let variable_name = extract("ASSET_indicator_params_tf");
```

Rules:

- The key must be a string literal.
- The statement must start with `let` and end with `;`.
- The variable name must contain only letters, digits, and underscores, and should not start with a digit.
- Dynamic keys, single quotes, `const`, destructuring, and calling `extract()` inside another expression are not expanded.

If the indicator value is missing or not warmed up yet, the expanded code does `return;`, so the script skips the tick without placing an order.

**Single-value indicators** (RSI, EMA, DEMA, TEMA, SMA, ATR, ADX, SMA on RSI, ROC, OBV, VWAP Deviation, CCI, Volume MA, Historical Volatility):

```rust
let rsi = extract("BTC_rsi_14_15m");
```

Expands to:

```rust
let rsi = indicators["BTC_rsi_14_15m"];
if rsi == () { return; }
let rsi_value = as_f64(rsi.value);      // the numeric value
let rsi_on_close = rsi.on_close;        // true if from a closed candle
let rsi_ts = rsi.ts;                    // candle close timestamp (ms)
```

After `extract()`, use `rsi_value` directly in your logic.

**Stochastic RSI:**

```rust
let stoch = extract("SOL_stochRsi_14_3_3_15m");
```

Expands with:

```rust
let stoch_k = ...       // K line
let stoch_d = ...       // D line
let stoch_on_close = ...
let stoch_ts = ...
```

**EMA Cross:**

```rust
let ema = extract("BTC_emaCross_9_21_15m");
```

Expands with:

```rust
let ema_short = ...     // short EMA value
let ema_long = ...      // long EMA value
let ema_trend = ...     // true if short > long (bool)
let ema_on_close = ...
let ema_ts = ...
```

**MACD:**

```rust
let macd = extract("BTC_macd_12_26_9_15m");
```

Expands with:

```rust
let macd_macd = ...        // MACD line
let macd_signal = ...      // signal line
let macd_histogram = ...   // histogram
let macd_on_close = ...
let macd_ts = ...
```

**Ichimoku:**

```rust
let ichi = extract("ETH_ichimoku_9_26_52_1h");
```

Expands with:

```rust
let ichi_tenkan = ...
let ichi_kijun = ...
let ichi_span_a = ...
let ichi_span_b = ...
let ichi_chikou = ...
let ichi_on_close = ...
let ichi_ts = ...
```

**Bollinger Bands:**

```rust
let bb = extract("SOL_bollinger_20_2_15m");
```

Expands with:

```rust
let bb_upper = ...
let bb_mid = ...
let bb_lower = ...
let bb_width = ...         // band width (%)
let bb_on_close = ...
let bb_ts = ...
```

The variable names are always `{your_name}_{field}`. Clicking an indicator badge in the editor inserts the full asset-specific `extract()` call for you. If an asset symbol contains `:`, `extract()` normalizes it to `_` during lookup.

### Closed-Candle Guards

Every `extract()` expansion creates `{name}_on_close` and `{name}_ts`:

- `{name}_on_close` is `true` when the indicator was updated after a candle close.
- `{name}_ts` is the previous closed candle timestamp in milliseconds, or `0` before the tracker has a closed candle.

Use these fields to avoid trading on in-progress candle updates:

```rust
let rsi = extract("self_rsi_14_15m");

if !rsi_on_close {
    return;
}

if rsi_value < 30.0 {
    return open_market(LONG, margin_pct(50.0));
}
```

In cross-asset strategies, pair `_ts` with state so a condition only fires once per market-asset candle:

State declarations:

```
last_market_ts = 0
```

Script:

```rust
let rsi = extract("self_rsi_14_15m");

if !rsi_on_close || rsi_ts == last_market_ts {
    return;
}

last_market_ts = rsi_ts;

if rsi_value < 30.0 {
    return open_market(LONG, margin_pct(50.0));
}
```

---

## Price Object

`last_price` is the candle for the current evaluation tick:

| Field | Type | Description |
|-------|------|-------------|
| `.open` | `f64` | Open price |
| `.high` | `f64` | High price |
| `.low` | `f64` | Low price |
| `.close` | `f64` | Close price |
| `.vlm` | `f64` | Volume |
| `.open_time` | `i64` | Candle open timestamp (ms) |
| `.close_time` | `i64` | Candle close timestamp (ms) |

Use `last_price.close` for market-relative calculations only in single-asset strategies, or when you are certain the current evaluation was triggered by the traded market. A BTC update can evaluate a SOL-attached strategy if BTC indicators are configured, so cross-asset strategies should avoid deriving SOL limit prices from `last_price.close`.

---

## Open Position Info

Available in `on_open` as `open_position`:

| Field | Type | Description |
|-------|------|-------------|
| `.side` | `Side` | `LONG` or `SHORT` |
| `.size` | `f64` | Position size in asset units |
| `.entry_px` | `f64` | Average entry price |
| `.open_time` | `i64` | Position open timestamp (ms) |

Compare side values directly:

```rust
if open_position.side == LONG {
    // long position
} else if open_position.side == SHORT {
    // short position
}
```

---

## Busy Reason

Available in `on_busy` as `busy_reason`:

```rust
busy_reason.is_opening()    // true if waiting for an open order to fill
busy_reason.is_closing()    // true if waiting for a close order to fill
```

Timeout details are not exposed as fields in Rhai; use these helpers to branch while busy.

---

## State Declarations

Declare persistent variables in the **State Variables** box in the editor. One per line, `name = default`:

```
count = 0
last_signal = "none"
prev_uptrend = null
```

These variables are automatically initialized on the first tick and persist across ticks. Use them as bare locals in your scripts — no `state["..."]` boilerplate.

```rust
// on_idle
count += 1;
if count > 10 {
    open_market(LONG, margin_pct(50.0))
}
```

Supported default types: numbers, strings (`"..."`), booleans (`true`/`false`), and `null` (Rhai `()` — useful for "not set yet").

State names must be valid Rhai-style identifiers. Use letters, digits, and underscores, and start with a letter or underscore:

```
trade_count = 0
last_signal = "none"
_armed_once = false
```

Avoid names that collide with context variables or constants such as `free_margin`, `last_price`, `LONG`, or `MIN15`.

State is shared by `on_idle`, `on_open`, and `on_busy`. At the start of every script evaluation, each declared variable is loaded from the persistent state map or its default. After evaluation, the latest value is written back. This happens even if the script returns no intent.

A `null` default lets you check whether a value has been assigned:

```rust
if prev_uptrend != () {
    // only runs after prev_uptrend has been set to a real value
}
```

State resets when the strategy is reloaded, the bot restarts, or a backtest engine is reset.

---

## Example Strategies

All examples below use asset-prefixed indicator keys or the `self_` market alias.

### BTC RSI Mean Reversion

**Indicators:** BTC RSI(14) on 15m

**on_idle:**
```rust
let rsi = extract("BTC_rsi_14_15m");

if !rsi_on_close {
    return;
}

if rsi_value < 30.0 {
    open_market(LONG, margin_pct(50.0), triggers(3.0, 2.0))
} else if rsi_value > 70.0 {
    open_market(SHORT, margin_pct(50.0), triggers(3.0, 2.0))
}
```

**on_open / on_busy:** empty (TP/SL triggers handle the exit)

### Cross-Asset Armed Entry

Attach this strategy to **SOL**. It uses **BTC** as a higher-timeframe filter and `self_...` for the attached market's local trend / exit logic.

**Indicators:** BTC RSI(12) on 1h, SOL EMA Cross(9, 21) on 15m, SOL RSI(14) on 15m

**State declarations:**
```
prev_uptrend = null
last_entry_ts = 0
last_exit_ts = 0
```

**on_idle:**
```rust
let btc_rsi_1h = extract("BTC_rsi_12_1h");
let market_ema = extract("self_emaCross_9_21_15m");

if !market_ema_on_close || market_ema_ts == last_entry_ts {
    return;
}

last_entry_ts = market_ema_ts;

if is_armed > 0 {
    if !prev_uptrend && market_ema_trend {
        prev_uptrend = market_ema_trend;
        return open_market(LONG, margin_pct(80.0), sl_only(28.0));
    }
    if prev_uptrend && !market_ema_trend {
        prev_uptrend = market_ema_trend;
        return open_market(SHORT, margin_pct(80.0), sl_only(28.0));
    }
} else if btc_rsi_1h_value < 60.0 && !market_ema_trend {
    prev_uptrend = market_ema_trend;
    return arm(timedelta(MIN15, 1));
} else if btc_rsi_1h_value > 70.0 && market_ema_trend {
    prev_uptrend = market_ema_trend;
    return arm(timedelta(MIN15, 1));
}

prev_uptrend = market_ema_trend;
```

**on_open:**
```rust
let btc_rsi_1h = extract("BTC_rsi_12_1h");
let market_rsi_15m = extract("self_rsi_14_15m");

if !market_rsi_15m_on_close || market_rsi_15m_ts == last_exit_ts {
    return;
}

last_exit_ts = market_rsi_15m_ts;

let elapsed = last_price.open_time - open_position.open_time;

if open_position.side == LONG {
    if market_rsi_15m_value >= 68.0 || (elapsed > timedelta(MIN15, 2) && btc_rsi_1h_value < 33.0) {
        return flatten_market();
    }
} else {
    if market_rsi_15m_value <= 38.0 || (elapsed > timedelta(MIN15, 2) && btc_rsi_1h_value > 58.0) {
        return flatten_market();
    }
}
```

**on_busy:** empty (wait for fill)

### Limit Order with Timeout

**Indicators:** BTC RSI(14) on 5m

**on_idle:**
```rust
let rsi = extract("BTC_rsi_14_5m");

if !rsi_on_close {
    return;
}

if rsi_value < 25.0 {
    // Place a limit buy 0.1% below current price.
    // If not filled in 15 minutes, cancel tracked resting orders and exit any exposure.
    let px = last_price.close * 0.999;
    open_limit(LONG, margin_pct(40.0), px, timeout(CANCEL, timedelta(MIN5, 3)), triggers(3.0, 1.5))
}
```

**on_open / on_busy:** empty

---

## Backtesting

Test your strategies against historical data before deploying. Configure:

- **Strategy** -- selected Rhai scripts, state declarations, and indicator set
- **Traded asset** -- the market being simulated
- **Time range** -- start and end timestamps
- **Resolution** -- candle timeframe for simulation
- **Margin & Leverage** -- initial capital and leverage
- **Fees** -- taker/maker fee in basis points
- **Funding rate** -- simulated funding rate per 8h

Results include equity curve, trade list, win rate, max drawdown, and position snapshots.

---

## Scripting Sandbox Limits

Scripts run inside a sandboxed Rhai engine with the following safety limits:

| Limit | Value |
|-------|-------|
| Max operations | 100,000 |
| Max expression depth | 64 |
| Max string size | 4,096 bytes |
| Max array size | 1,024 elements |
| Max map size | 256 entries |
| Min order value | $10 USDC |

---

## Authentication

Connect with any Ethereum-compatible wallet. The flow is:

1. Request a nonce for your address
2. Sign the nonce with your wallet
3. Receive a JWT for authenticated API access

Your Hyperliquid API key is encrypted at rest and only decrypted server-side when your bot needs to execute trades.

---

## Quick Reference

```
Sides:      LONG  SHORT
Timeframes: MIN1 MIN3 MIN5 MIN15 MIN30 HOUR1 HOUR2 HOUR4 HOUR12 DAY1
Timeout:    FORCE  CANCEL

-- Sizing --
margin_pct(%)       margin_amount($)       raw_size(units)

-- Triggers --
triggers(tp%, sl%)  tp_only(tp%)           sl_only(sl%)

-- Timeouts --
timeout(action, timedelta(timeframe, count))

-- Open --
open_market(side, size)
open_market(side, size, triggers)
open_limit(side, size, px)
open_limit(side, size, px, triggers)
open_limit(side, size, px, timeout)
open_limit(side, size, px, timeout, triggers)

-- Close --
flatten_market()
flatten_limit(px)
flatten_limit(px, timeout)
reduce_market(size)
reduce_limit(size, px)
reduce_limit(size, px, timeout)

-- Control --
arm(timedelta)      disarm()               abort()

-- Debug --
print("message")

-- Indicators --
Key format: {ASSET}_{indicator}_{params}_{tf}
Market alias: self_{indicator}_{params}_{tf}
extract shape: let name = extract("KEY");
extract("BTC_rsi_14_15m")  →  {name}_value, {name}_on_close, {name}_ts
                              emaCross: {name}_short, {name}_long, {name}_trend
                              stochRsi: {name}_k, {name}_d
                              macd: {name}_macd, {name}_signal, {name}_histogram
                              ichimoku: {name}_tenkan, {name}_kijun, {name}_span_a, {name}_span_b, {name}_chikou
                              bollinger: {name}_upper, {name}_mid, {name}_lower, {name}_width

-- State --
Declare in State Variables box:   count = 0 | flag = false | x = null
Use as bare locals:               count += 1

-- Compile behavior --
Strict variables: typos and undeclared names fail on save.
Empty return / () / trailing semicolon: no trade intent.
```
