#![allow(unused_variables)]
use super::*;
use TimeFrame::*;
use Value::*;

const SMA_RSI_LONG_THRESH: f64 = 40.0;
const SMA_RSI_SHORT_THRESH: f64 = 60.0;
const STOCH_LOW: f64 = 20.0;
const STOCH_HIGH: f64 = 80.0;
const STOCH_ARM_LOW: f64 = 30.0;
const STOCH_ARM_HIGH: f64 = 70.0;
const PRICE_OFFSET: f64 = 0.002;
const ENTRY_MARGIN_PCT: f64 = 95.0;

pub struct SmaRsiScalpTest {
    sma_rsi_5m: IndexId,
    stoch_rsi_1m: IndexId,
    armed_side: Option<Side>,
}

impl SmaRsiScalpTest {
    pub fn init() -> Self {
        let inds = Self::required_indicators_static();
        Self {
            sma_rsi_5m: inds[0],
            stoch_rsi_1m: inds[1],
            armed_side: None,
        }
    }
}

impl NeedsIndicators for SmaRsiScalpTest {
    fn required_indicators_static() -> Vec<IndexId> {
        vec![
            (
                IndicatorKind::SmaOnRsi {
                    periods: 12,
                    smoothing_length: 10,
                },
                Min5,
            ),
            (
                IndicatorKind::StochRsi {
                    periods: 14,
                    k_smoothing: Some(3),
                    d_smoothing: Some(3),
                },
                Min1,
            ),
        ]
    }
}

impl Strat for SmaRsiScalpTest {
    fn required_indicators(&self) -> Vec<IndexId> {
        Self::required_indicators_static()
    }

    fn on_idle(&mut self, ctx: StratContext, armed: Armed) -> Option<Intent> {
        let StratContext {
            lev,
            last_price,
            indicators,
            ..
        } = ctx;

        if last_price.close <= 0.0 {
            return None;
        }

        if armed.is_none() {
            self.armed_side = None;
            let (k, d) = match indicators.get(&self.stoch_rsi_1m)?.value {
                StochRsiValue { k, d } => (k, d),
                _ => return None,
            };

            if k < STOCH_ARM_LOW && d < STOCH_ARM_LOW {
                self.armed_side = Some(Side::Long);
                return Some(Intent::Arm(timedelta!(Min1, 9)));
            }

            if k > STOCH_ARM_HIGH && d > STOCH_ARM_HIGH {
                self.armed_side = Some(Side::Short);
                return Some(Intent::Arm(timedelta!(Min1, 9)));
            }

            return None;
        }

        let expected_side = match self.armed_side {
            Some(side) => side,
            None => return Some(Intent::Disarm),
        };

        let sma_rsi = match indicators.get(&self.sma_rsi_5m)?.value {
            SmaRsiValue(v) => v,
            _ => return None,
        };

        let size = SizeSpec::MarginPct(ENTRY_MARGIN_PCT);
        let ttl = TimeoutInfo {
            action: OnTimeout::Force,
            duration: timedelta!(Min1, 2),
        };

        match expected_side {
            Side::Short if sma_rsi > SMA_RSI_SHORT_THRESH => {
                let limit_px = last_price.close * (1.0 + PRICE_OFFSET);
                self.armed_side = None;
                return Some(Intent::open_limit(
                    Side::Short,
                    size,
                    limit_px,
                    Some(ttl),
                    None,
                ));
            }
            Side::Long if sma_rsi < SMA_RSI_LONG_THRESH => {
                let limit_px = last_price.close * (1.0 - PRICE_OFFSET);
                self.armed_side = None;
                return Some(Intent::open_limit(
                    Side::Long,
                    size,
                    limit_px,
                    Some(ttl),
                    None,
                ));
            }
            _ => {}
        }

        None
    }

    fn on_open(&mut self, ctx: StratContext, open_pos: &OpenPosInfo) -> Option<Intent> {
        let StratContext {
            last_price,
            indicators,
            ..
        } = ctx;

        let (k, d) = match indicators.get(&self.stoch_rsi_1m)?.value {
            StochRsiValue { k, d } => (k, d),
            _ => return None,
        };

        if last_price.close <= 0.0 {
            return None;
        }

        let ttl = TimeoutInfo {
            action: OnTimeout::Force,
            duration: timedelta!(Min1, 2),
        };

        match open_pos.side {
            Side::Long if k > STOCH_HIGH && d > STOCH_HIGH => {
                return Some(Intent::flatten_limit(
                    last_price.close * (1.0 + PRICE_OFFSET),
                    Some(ttl),
                ));
            }
            Side::Short if k < STOCH_LOW && d < STOCH_LOW => {
                return Some(Intent::flatten_limit(
                    last_price.close * (1.0 - PRICE_OFFSET),
                    Some(ttl),
                ));
            }
            _ => {}
        }

        None
    }

    fn on_busy(&mut self, _ctx: StratContext, _busy: BusyType) -> Option<Intent> {
        None
    }
}
