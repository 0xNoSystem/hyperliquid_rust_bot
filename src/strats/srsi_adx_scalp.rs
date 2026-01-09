#![allow(unused_variables)]
use super::*;
use TimeFrame::*;
use Value::*;

pub struct SrsiAdxScalp {
    rsi_1h: IndexId,
    sma_rsi_1h: IndexId,
    adx_15m: IndexId,
    prev_rsi_above_sma: Option<bool>,
}

impl SrsiAdxScalp {
    pub fn init() -> Self {
        let inds = Self::required_indicators_static();
        Self {
            rsi_1h: inds[0],
            sma_rsi_1h: inds[1],
            adx_15m: inds[2],
            prev_rsi_above_sma: None,
        }
    }
}

impl NeedsIndicators for SrsiAdxScalp {
    fn required_indicators_static() -> Vec<IndexId> {
        vec![
            (IndicatorKind::Rsi(14), Hour1),
            (
                IndicatorKind::SmaOnRsi {
                    periods: 14,
                    smoothing_length: 10,
                },
                Hour1,
            ),
            (
                IndicatorKind::Adx {
                    periods: 10,
                    di_length: 10,
                },
                Min15,
            ),
        ]
    }
}

impl Strat for SrsiAdxScalp {
    fn required_indicators(&self) -> Vec<IndexId> {
        Self::required_indicators_static()
    }

    // =========================
    // IDLE: setup + trigger
    // =========================
    fn on_idle(&mut self, ctx: StratContext, armed: Armed) -> Option<Intent> {
        let StratContext {
            free_margin,
            lev,
            last_price,
            indicators,
        } = ctx;

        // ADX logic only evaluated on ADX close when flat (same as vanilla)
        let adx_val = indicators.get(&self.adx_15m)?;
        if !adx_val.on_close {
            return None;
        }

        let rsi_1h = match indicators.get(&self.rsi_1h)?.value {
            RsiValue(v) => v,
            _ => return None,
        };

        let sma_rsi_1h = match indicators.get(&self.sma_rsi_1h)?.value {
            SmaRsiValue(v) => v,
            _ => return None,
        };

        let adx_15m = match adx_val.value {
            AdxValue(v) => v,
            _ => return None,
        };

        let rsi_above_sma = rsi_1h > sma_rsi_1h;

        // -------------------------
        // Trigger while armed
        // -------------------------
        if armed.is_some() {
            if let Some(prev) = self.prev_rsi_above_sma
                && !prev && rsi_above_sma {
                    let size = SizeSpec::MarginPct(95.0);
                    let limit_px = calc_entry_px(
                        Side::Long,
                        0.3,
                        last_price.close,
                        lev,
                    );

                    return Some(Intent::open_limit(
                        Side::Long,
                        size,
                        limit_px,
                        None,
                        None,
                    ));
            }
        } else {
            // -------------------------
            // Arm setup window
            // -------------------------
            if adx_15m > 48.0 && !rsi_above_sma {
                return Some(Intent::Arm(timedelta!(Hour1, 3)));
            }
        }

        self.prev_rsi_above_sma = Some(rsi_above_sma);
        None
    }

    // =========================
    // OPEN: position management
    // =========================
    fn on_open(&mut self, ctx: StratContext, open_pos: &OpenPosInfo) -> Option<Intent> {
        let StratContext {
            lev,
            last_price,
            indicators,
            ..
        } = ctx;

        let rsi_1h = match indicators.get(&self.rsi_1h)?.value {
            RsiValue(v) => v,
            _ => return None,
        };

        let sma_rsi_1h = match indicators.get(&self.sma_rsi_1h)?.value {
            SmaRsiValue(v) => v,
            _ => return None,
        };

        // -------- SL (once, engine dedups) --------
        let sl_px = calc_exit_px(
            open_pos.side,
            TriggerKind::Sl,
            open_pos.entry_px,
            0.3,
            lev,
        );

        if rsi_1h < 60.0 {
            return Some(Intent::reduce_limit_order(
                SizeSpec::RawSize(open_pos.size),
                sl_px,
                None,
            ));
        }

        // -------- TP / exit --------
        if rsi_1h > 60.0 && (rsi_1h - sma_rsi_1h) < (rsi_1h * 0.1) {
            let tp_px = calc_exit_px(
                open_pos.side,
                TriggerKind::Tp,
                open_pos.entry_px,
                0.003,
                lev,
            );

            return Some(Intent::flatten_limit(tp_px, None));
        }

        None
    }

    // =========================
    // BUSY: no abort logic
    // =========================
    fn on_busy(&mut self, _ctx: StratContext, _busy: BusyType) -> Option<Intent> {
        None
    }
}

pub fn calc_entry_px(side: Side, delta: f64, ref_px: f64, lev: usize) -> f64 {
    assert!(lev > 0);
    assert!(delta >= 0.0);

    let d = delta / lev as f64;

    match side {
        Side::Long => ref_px * (1.0 - d),
        Side::Short => ref_px * (1.0 + d),
    }
}

pub fn calc_exit_px(side: Side, exit: TriggerKind, entry_px: f64, delta: f64, lev: usize) -> f64 {
    assert!(lev > 0);
    assert!(delta >= 0.0);

    let d = match exit {
        TriggerKind::Tp => delta / lev as f64,
        TriggerKind::Sl => delta.min(1.0) / lev as f64,
    };

    match (side, exit) {
        (Side::Long, TriggerKind::Tp) => entry_px * (1.0 + d),
        (Side::Long, TriggerKind::Sl) => entry_px * (1.0 - d),
        (Side::Short, TriggerKind::Tp) => entry_px * (1.0 - d),
        (Side::Short, TriggerKind::Sl) => entry_px * (1.0 + d),
    }
}
