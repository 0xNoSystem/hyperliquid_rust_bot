use super::*;
use TimeFrame::*;
use Value::*;

#[derive(Clone, Debug, PartialEq)]
pub struct SrsiAdxScalp {
    rsi_1h: IndexId,
    sma_rsi_1h: IndexId,
    adx_15m: IndexId,
    prev_rsi_above_sma: Option<bool>,
    active_window_start: Option<u64>,
    sl_set: bool,
    closing: bool,
}

impl SrsiAdxScalp {
    pub fn init() -> Self {
        let inds = Self::required_indicators_static();
        SrsiAdxScalp {
            rsi_1h: inds[0],
            sma_rsi_1h: inds[1],
            adx_15m: inds[2],
            prev_rsi_above_sma: None,
            active_window_start: None,
            sl_set: false,
            closing: false,
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

    fn on_tick(
        &mut self,
        snapshot: ValuesMap,
        price: f64,
        params: &ExecParams,
        now: u64,
    ) -> Option<EngineOrder> {
        let margin = params.free_margin();
        let lev = params.lev;
        let open_pos = params.open_pos;

        let max_size = (margin * lev as f64) / price;

        let rsi_1h_value = match snapshot.get(&self.rsi_1h)?.value {
            RsiValue(v) => v,
            _ => return None,
        };

        let sma_rsi_1h_value = match snapshot.get(&self.sma_rsi_1h)?.value {
            SmaRsiValue(v) => v,
            _ => return None,
        };

        let adx_15m_value = match snapshot.get(&self.adx_15m)?.value {
            AdxValue(v) => v,
            _ => return None,
        };

        let rsi_above_sma = rsi_1h_value > sma_rsi_1h_value;

        let order = (|| {
            if self.closing {
                return None;
            };

            if let Some(open) = open_pos {
                if !self.sl_set {
                    let trigger_px =
                        calc_exit_px(open.side, TriggerKind::Sl, 0.3, open.entry_px, lev);
                    self.sl_set = true;
                    return Some(EngineOrder::new_sl(open.size, trigger_px));
                }
                if rsi_1h_value > 60.0 && (rsi_1h_value - sma_rsi_1h_value < (rsi_1h_value * 0.1)) {
                    let limit_px = calc_exit_px(open.side, TriggerKind::Tp, 0.003, open.entry_px, lev);
                    self.closing = true;
                    return Some(EngineOrder::new_limit_close(open.size, limit_px, None));
                }
            }
            let start = self.active_window_start?;

            if now - start >= timedelta!(Min15, 3) {
                self.active_window_start = None;
                return None;
            }

            let prev_rsi_above_sma = self.prev_rsi_above_sma?;
            if prev_rsi_above_sma || !rsi_above_sma {
                return None;
            }

            if open_pos.is_none() {
                let size = max_size * 0.95;
                if size * price < MIN_ORDER_VALUE {
                    return None;
                }
                self.active_window_start = None;
                let limit_px = calc_entry_px(Side::Long, price, 0.3, lev);
                return Some(EngineOrder::limit_open_long(size, limit_px, None));
            }
            None
        })();

        //activate trade window if adx > 48
        if self.active_window_start.is_none() && open_pos.is_none() {
            self.closing = false;
            self.sl_set = false;
            if adx_15m_value > 48.0 && !rsi_above_sma {
                self.active_window_start = Some(now);
            }
        }

        self.prev_rsi_above_sma = Some(rsi_above_sma);
        order
    }
}

pub fn calc_entry_px(side: Side, ref_px: f64, delta: f64, lev: usize) -> f64 {
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
