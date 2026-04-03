# Hyperliquid Trading Terminal

A multi-user perpetual futures trading terminal for [Hyperliquid](https://hyperliquid.xyz), featuring a scripting engine that lets you write, test, and deploy automated trading strategies from your browser.

Built with a Rust backend (Axum + WebSocket) and a React frontend. Strategies are written in **Rhai** (a Rust-native scripting language) and execute server-side with sandboxed resource limits.

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
3. You write a strategy using the scripting API (or use an existing one).
4. The engine evaluates your scripts on every candle tick and executes the resulting orders on Hyperliquid.

---

## Strategy Scripting API

A strategy is three scripts that run at different stages of a trade lifecycle:

| Script | When it runs | Extra variable |
|--------|-------------|----------------|
| **`on_idle`** | No position is open | `is_armed` -- expiry timestamp (or `-1` if not armed) |
| **`on_open`** | A position is open | `open_position` -- current position info |
| **`on_busy`** | An order is pending (waiting for fill) | `busy_reason` -- opening or closing info |

Each script can return an **Intent** (a trading action) or return nothing (do nothing).

### Context Variables

These are available in **all three** scripts:

| Variable | Type | Description |
|----------|------|-------------|
| `free_margin` | `f64` | Available margin in USDC |
| `lev` | `i64` | Current leverage multiplier |
| `last_price` | `Price` | Latest candle data |
| `indicators` | `Map` | Indicator values (use `extract()` instead of accessing directly) |

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

**Order type**
```
TAKER           -- market order execution
```

**Timeout actions**
```
FORCE           -- force-execute at market when timeout expires
CANCEL          -- cancel the order when timeout expires
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

---

## Indicators

Add indicators to your strategy through the editor UI. Each indicator is bound to a timeframe and accessed from the `indicators` map.

### Available Indicators

| Indicator | Key format | Parameters |
|-----------|-----------|------------|
| RSI | `rsi_{periods}_{tf}` | periods |
| EMA | `ema_{periods}_{tf}` | periods |
| SMA | `sma_{periods}_{tf}` | periods |
| ATR | `atr_{periods}_{tf}` | periods |
| ADX | `adx_{periods}_{di_length}_{tf}` | periods, DI length |
| Stochastic RSI | `stochRsi_{periods}_{k}_{d}_{tf}` | periods, K smoothing, D smoothing |
| SMA on RSI | `smaRsi_{periods}_{smoothing}_{tf}` | periods, smoothing length |
| EMA Cross | `emaCross_{short}_{long}_{tf}` | short period, long period |
| Volume MA | `volMa_{periods}_{tf}` | periods |
| Historical Volatility | `histVol_{periods}_{tf}` | periods |

### The `extract()` Macro

Use `extract()` to access an indicator. It handles the lookup, null guard, and value unpacking automatically. You write one line and get several ready-to-use variables.

**Single-value indicators** (RSI, EMA, SMA, ATR, ADX, SMA on RSI, Volume MA, Historical Volatility):

```rust
let rsi = extract("rsi_14_15m");
```

Expands to:

```rust
let rsi = indicators["rsi_14_15m"];
if rsi == () { return; }
let rsi_value = rsi.value.as_f64();     // the numeric value
let rsi_on_close = rsi.on_close;        // true if from a closed candle
let rsi_ts = rsi.ts;                    // candle close timestamp (ms)
```

After `extract()`, use `rsi_value` directly in your logic.

**Stochastic RSI:**

```rust
let stoch = extract("stochRsi_14_0_0_15m");
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
let ema = extract("emaCross_9_21_15m");
```

Expands with:

```rust
let ema_short = ...     // short EMA value
let ema_long = ...      // long EMA value
let ema_trend = ...     // true if short > long (bool)
let ema_on_close = ...
let ema_ts = ...
```

The variable names are always `{your_name}_{field}`. Clicking an indicator badge in the editor inserts the `extract()` call for you.

---

## Price Object

`last_price` is the latest candle:

| Field | Type | Description |
|-------|------|-------------|
| `.open` | `f64` | Open price |
| `.high` | `f64` | High price |
| `.low` | `f64` | Low price |
| `.close` | `f64` | Close price |
| `.vlm` | `f64` | Volume |
| `.open_time` | `i64` | Candle open timestamp (ms) |
| `.close_time` | `i64` | Candle close timestamp (ms) |

---

## Open Position Info

Available in `on_open` as `open_position`:

| Field | Type | Description |
|-------|------|-------------|
| `.side` | `Side` | `LONG` or `SHORT` |
| `.size` | `f64` | Position size in asset units |
| `.entry_px` | `f64` | Average entry price |
| `.open_time` | `i64` | Position open timestamp (ms) |

---

## Busy Reason

Available in `on_busy` as `busy_reason`:

```rust
busy_reason.is_opening()    // true if waiting for an open order to fill
busy_reason.is_closing()    // true if waiting for a close order to fill
```

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

A `null` default lets you check whether a value has been assigned:

```rust
if prev_uptrend != () {
    // only runs after prev_uptrend has been set to a real value
}
```

State resets when the strategy is reloaded or the bot restarts.

---

## Example Strategies

### Simple RSI Mean Reversion

**Indicators:** RSI(14) on 15m

**on_idle:**
```rust
let rsi = extract("rsi_14_15m");

if rsi_value < 30.0 {
    open_market(LONG, margin_pct(50.0), triggers(3.0, 2.0))
} else if rsi_value > 70.0 {
    open_market(SHORT, margin_pct(50.0), triggers(3.0, 2.0))
}
```

**on_open / on_busy:** empty (TP/SL triggers handle the exit)

### EMA Cross + RSI with Armed Entry

**Indicators:** RSI(12) on 1h, EMA Cross(9, 21) on 15m

**State declarations:**
```
prev_uptrend = null
```

**on_idle:**
```rust
let rsi_1h = extract("rsi_12_1h");
let ema = extract("emaCross_9_21_15m");

if is_armed > 0 {
    if !prev_uptrend && ema_trend {
        prev_uptrend = ema_trend;
        return open_market(LONG, margin_pct(80.0), sl_only(28.0));
    }
    if prev_uptrend && !ema_trend {
        prev_uptrend = ema_trend;
        return open_market(SHORT, margin_pct(80.0), sl_only(28.0));
    }
} else if rsi_1h_value < 60.0 && !ema_trend {
    prev_uptrend = ema_trend;
    return arm(timedelta(MIN15, 1));
} else if rsi_1h_value > 70.0 && ema_trend {
    prev_uptrend = ema_trend;
    return arm(timedelta(MIN15, 1));
}

prev_uptrend = ema_trend;
```

**on_open:**
```rust
let rsi_1h = extract("rsi_12_1h");
let rsi_15m = extract("rsi_14_15m");

let elapsed = last_price.open_time - open_position.open_time;

if open_position.side == LONG {
    if rsi_15m_value >= 68.0 || (elapsed > timedelta(MIN15, 2) && rsi_1h_value < 33.0) {
        return flatten_limit(last_price.close * 1.003);
    }
} else {
    if rsi_15m_value <= 38.0 || (elapsed > timedelta(MIN15, 2) && rsi_1h_value > 58.0) {
        return flatten_limit(last_price.close * 0.997);
    }
}
```

**on_busy:** empty (wait for fill)

### Limit Order with Timeout

**Indicators:** RSI(14) on 5m

**on_idle:**
```rust
let rsi = extract("rsi_14_5m");

if rsi_value < 25.0 {
    // place a limit buy 0.1% below current price
    // if not filled in 15 minutes, force-execute at market
    let px = last_price.close * 0.999;
    open_limit(LONG, margin_pct(40.0), px, timeout(FORCE, timedelta(MIN5, 3)), triggers(3.0, 1.5))
}
```

**on_open / on_busy:** empty

---

## Backtesting

Test your strategies against historical data before deploying. Configure:

- **Asset** -- any Hyperliquid-listed perpetual
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

-- Indicators --
extract("key")  →  {name}_value, {name}_on_close, {name}_ts
                   emaCross: {name}_short, {name}_long, {name}_trend
                   stochRsi: {name}_k, {name}_d

-- State --
Declare in State Variables box:   count = 0 | flag = false | x = null
Use as bare locals:               count += 1
```
