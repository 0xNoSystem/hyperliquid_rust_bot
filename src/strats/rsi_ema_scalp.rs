use super::*;
use TimeFrame::*;
use Value::*;

pub struct RsiEmaScalp {
    rsi_1h: IndexId,
    rsi_15m: IndexId,
    ema_cross_15m: IndexId,
    active_window_start: Option<u64>, //ms
    prev_fast_above: Option<bool>,
    limit_close_set: bool,
}

impl RsiEmaScalp {
    pub fn init() -> Self {
        let inds = Self::required_indicators_static();
        RsiEmaScalp {
            rsi_1h: inds[0],
            ema_cross_15m: inds[1],
            rsi_15m: inds[2],
            active_window_start: None,
            prev_fast_above: None,
            limit_close_set: false,
        }
    }
}

impl NeedsIndicators for RsiEmaScalp {
    fn required_indicators_static() -> Vec<IndexId> {
        vec![
            (IndicatorKind::Rsi(12), TimeFrame::Hour1),
            (
                IndicatorKind::EmaCross { short: 9, long: 21 },
                TimeFrame::Min15,
            ),
            (IndicatorKind::Rsi(14), TimeFrame::Min15),
        ]
    }
}

impl Strat for RsiEmaScalp {
    fn required_indicators(&self) -> Vec<IndexId> {
        Self::required_indicators_static()
    }

    fn on_tick(&mut self, ctx: StratContext) -> Option<EngineOrder> {
        let StratContext {
            free_margin,
            lev,
            last_price,
            indicators,
            tick_time,
            open_pos,
        } = ctx;

        let max_size = (free_margin * lev as f64) / last_price;

        let rsi_1h_value = match indicators.get(&self.rsi_1h)?.value {
            RsiValue(v) => v,
            _ => return None,
        };

        let rsi_15m_value = match indicators.get(&self.rsi_15m)?.value {
            RsiValue(v) => v,
            _ => return None,
        };

        let (_fast, _slow, uptrend) = match indicators.get(&self.ema_cross_15m)?.value {
            EmaCrossValue { short, long, trend } => (short, long, trend),
            _ => return None,
        };
        let order = (|| {
            if let Some(open) = open_pos {
                println!("HEREEEE");
                if !self.limit_close_set
                    && (rsi_15m_value >= 50.0
                        || ((tick_time - open.open_time > timedelta!(Min15, 1))
                            && rsi_1h_value < 35.0))
                {
                    self.active_window_start = None;
                    self.limit_close_set = true;
                    return Some(EngineOrder::new_limit_close(
                        open.size,
                        last_price * 1.003,
                        None,
                    ));
                }
            } else {
                self.limit_close_set = false;
            }
            let start = self.active_window_start?;

            if tick_time - start >= timedelta!(Hour1, 3) {
                self.active_window_start = None;
                return None;
            }

            let prev_uptrend = self.prev_fast_above?;
            if prev_uptrend || !uptrend {
                return None;
            }

            if open_pos.is_none() {
                if max_size * last_price < MIN_ORDER_VALUE {
                    return None;
                }
                self.active_window_start = None;
                let size = max_size * 0.9;
                return Some(EngineOrder::market_open_long(size));
            }
            None
        })();

        if self.active_window_start.is_none()
            && open_pos.is_none()
            && rsi_1h_value < 30.0
            && !uptrend
        {
            self.active_window_start = Some(tick_time);
        }

        self.prev_fast_above = Some(uptrend);
        order
    }
}
