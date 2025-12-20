use super::*;
use TimeFrame::*;
use Value::*;

#[derive(Clone, Debug, PartialEq)]
pub struct RsiEmaStrategy {
    rsi_1h: IndexId,
    rsi_5m: IndexId,
    ema_cross_15m: IndexId,
    active_window_start: Option<u64>, //ms
    waiting_for_cross: bool,
    prev_fast_above: Option<bool>,
    limit_close_set: bool,
}

impl RsiEmaStrategy {
    pub fn init() -> Self {
        let inds = Self::required_indicators_static();
        RsiEmaStrategy {
            rsi_1h: inds[0],
            ema_cross_15m: inds[1],
            rsi_5m: inds[2],
            active_window_start: None,
            waiting_for_cross: true,
            prev_fast_above: None,
            limit_close_set: false,
        }
    }
}

impl NeedsIndicators for RsiEmaStrategy {
    fn required_indicators_static() -> Vec<IndexId> {
        vec![
            (IndicatorKind::Rsi(8), TimeFrame::Min15),
            (
                IndicatorKind::EmaCross { short: 9, long: 21 },
                TimeFrame::Min1,
            ),
            (IndicatorKind::Rsi(10), TimeFrame::Min5),
        ]
    }
}

impl Strat for RsiEmaStrategy {
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
        let lev = params.lev as f64;
        let open_pos = params.open_pos;

        let max_size = (margin * lev) / price;

        let rsi_1h_value = match snapshot.get(&self.rsi_1h)?.value {
            RsiValue(v) => v,
            _ => return None,
        };

        let rsi_5m_value = match snapshot.get(&self.rsi_5m)?.value {
            RsiValue(v) => v,
            _ => return None,
        };

        let (_fast, _slow, uptrend) = match snapshot.get(&self.ema_cross_15m)?.value {
            EmaCrossValue { short, long, trend } => (short, long, trend),
            _ => return None,
        };
        let order = (|| {
            if let Some(open) = open_pos {
                if !self.limit_close_set
                    && (rsi_5m_value >= 50.0
                        || ((now - open.open_time > timedelta!(Min15, 1)) && rsi_1h_value < 35.0))
                {
                    self.active_window_start = None;
                    self.limit_close_set = true;
                    return Some(EngineOrder::new_limit_close(
                        open.size,
                        price * 1.003,
                        None,
                    ));
                }
            } else {
                self.limit_close_set = false;
            }
            let start = self.active_window_start?;

            if now - start >= timedelta!(Hour1, 3) {
                self.active_window_start = None;
                return None;
            }

            let prev_uptrend = self.prev_fast_above?;
            if prev_uptrend || !uptrend {
                return None;
            }

            if open_pos.is_none() {
                if max_size * price < MIN_ORDER_VALUE {
                    return None;
                }
                self.active_window_start = None;
                let size = max_size * 0.9;
                return Some(EngineOrder::market_open_long(size));
            }
            None
        })();

        if self.active_window_start.is_none() && open_pos.is_none() {
            if rsi_1h_value < 30.0 && !uptrend {
                self.active_window_start = Some(now);
            }
        }
        self.prev_fast_above = Some(uptrend);
        order
    }
}
