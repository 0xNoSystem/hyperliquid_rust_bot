#![allow(unused_variables)]
use super::*;
use TimeFrame::*;
use Value::*;

const RSI_THRESH: f64 = 28.0;
const ADX_THRESH: f64 = 35.0;
const NATR_THRESH: f64 = 0.03;

pub struct RsiChopSwing {
    rsi_1h: IndexId,
    adx_12h: IndexId,
    atr_1d: IndexId,
}

impl RsiChopSwing {
    pub fn init() -> Self {
        let inds = Self::required_indicators_static();
        Self {
            rsi_1h: inds[0],
            adx_12h: inds[1],
            atr_1d: inds[2],
        }
    }
}

impl NeedsIndicators for RsiChopSwing {
    fn required_indicators_static() -> Vec<IndexId> {
        vec![
            (IndicatorKind::Rsi(12), Hour1),
            (IndicatorKind::Adx { periods: 14, di_length: 10 }, Hour12),
            (IndicatorKind::Atr(14), Day1),
        ]
    }
}

impl Strat for RsiChopSwing {
    fn required_indicators(&self) -> Vec<IndexId> {
        Self::required_indicators_static()
    }

    fn on_idle(&mut self, ctx: StratContext, armed: Armed) -> Option<Intent> {
        let StratContext {
            free_margin,
            lev,
            last_price,
            indicators,
        } = ctx;

        // NOTE: last_price is a 1m candle (Price). Use close as "current price".
        let px = last_price.close;
        if px <= 0.0 {
            return None;
        }

        let max_size = (free_margin * lev as f64) / px;

        let rsi_1h_value = match indicators.get(&self.rsi_1h)?.value {
            RsiValue(v) => v,
            _ => return None,
        };

        let atr_1d_value = match indicators.get(&self.atr_1d)?.value {
            AtrValue(v) => v,
            _ => return None,
        };

        let adx_12h_value = match indicators.get(&self.adx_12h)?.value {
            AdxValue(v) => v,
            _ => return None,
        };

        let atr_normalized = atr_1d_value / px;

        // ---- Setup -> Arm (replaces active_window_start) ----
        // Vanilla behavior:
        //   If not in an active window and (atr_normalized > NATR && adx < ADX) and not opening:
        //      start window (10 hours)
        //
        // New behavior:
        //   If not armed, request Arm(10h) when setup is true.
        if armed.is_none() && (atr_normalized > NATR_THRESH && adx_12h_value < ADX_THRESH) {
            return Some(Intent::Arm(timedelta!(Hour1, 10)));
        }

        // ---- Trigger inside armed window -> Open ----
        // Vanilla behavior:
        //   While active_window_start exists (<=10h), if RSI extreme -> place limit open.
        //
        // New behavior:
        //   While armed (engine guarantees expiry), if RSI extreme -> open.
        if armed.is_some() {
            let size = max_size * 0.9;

            if rsi_1h_value < RSI_THRESH {
                // Long limit open slightly below
                let limit_px = px * 0.997;

                // Approximate TP/SL off intended entry px (closest equivalent to old entry_px-based TP/SL)
                let tp_sl = Some(Triggers {
                    tp: Some(limit_px * 1.03),
                    sl: Some(limit_px * 0.98),
                });

                // No per-order timeout in vanilla open; keep None.
                return Some(Intent::open_limit(
                    Side::Long,
                    SizeSpec::RawSize(size),
                    limit_px,
                    None,
                    tp_sl,
                ));
            }

            if rsi_1h_value > 100.0 - RSI_THRESH {
                // Short limit open slightly above
                let limit_px = px * 1.003;

                let tp_sl = Some(Triggers {
                    tp: Some(limit_px * 0.97),
                    sl: Some(limit_px * 1.02),
                });

                return Some(Intent::open_limit(
                    Side::Short,
                    SizeSpec::RawSize(size),
                    limit_px,
                    None,
                    tp_sl,
                ));
            }
        }

        None
    }

    fn on_open(&mut self, ctx: StratContext, open_pos: &OpenPosInfo) -> Option<Intent> {
        let StratContext {
            free_margin,
            lev,
            last_price,
            indicators,
        } = ctx;

        let px = last_price.close;
        if px <= 0.0 {
            return None;
        }

        let rsi_1h_value = match indicators.get(&self.rsi_1h)?.value {
            RsiValue(v) => v,
            _ => return None,
        };

        // Vanilla behavior:
        //   Long exit: if RSI > 52 -> limit close slightly above
        //   Short exit: if RSI < 48 -> limit close slightly below
        //
        // New behavior:
        //   Use Flatten(limit) with same price bias. Executor/engine handles lifecycle.
        match open_pos.side {
            Side::Long => {
                if rsi_1h_value > 52.0 {
                    return Some(Intent::flatten_limit(px * 1.001, None));
                }
            }
            Side::Short => {
                if rsi_1h_value < 48.0 {
                    return Some(Intent::flatten_limit(px * 0.999, None));
                }
            }
        }

        None
    }

    fn on_busy(&mut self, ctx: StratContext, busy_reason: BusyType) -> Option<Intent> {
        // Vanilla strat used internal `opening/closing` booleans.
        // In the new engine, Busy already means "execution in progress".
        //
        // Optional: you could choose to Abort under certain conditions,
        // but vanilla did not, so we keep it no-op.
        None
    }
}

