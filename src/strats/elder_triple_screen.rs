#![allow(unused_variables)]
use super::*;
use TimeFrame::*;
use Value::*;
use crate::{Price, ValuesMap};

const ADX_TREND_MIN: f64 = 20.0;
const ADX_TREND_EXIT: f64 = 18.0;
const RSI_LONG_MIN: f64 = 40.0;
const RSI_LONG_MAX: f64 = 50.0;
const RSI_SHORT_MIN: f64 = 50.0;
const RSI_SHORT_MAX: f64 = 60.0;
const STOCH_LOW: f64 = 20.0;
const STOCH_HIGH: f64 = 80.0;
const SL_ATR_MULT: f64 = 1.5;
const TP_ATR_MULT: f64 = 3.0;
const ENTRY_MARGIN_PCT: f64 = 95.0;

#[derive(Copy, Clone, Debug)]
struct StochState {
    k: f64,
    d: f64,
}

#[derive(Copy, Clone, Debug, PartialEq)]
enum CrossDir {
    Up,
    Down,
}

pub struct ElderTripleScreen {
    ema_200_h4: IndexId,
    adx_14_h4: IndexId,
    rsi_14_h1: IndexId,
    stoch_rsi_h1: IndexId,
    ema_cross_m5: IndexId,
    atr_m5: IndexId,
    vol_ma_m5: IndexId,
    prev_ema_trend: Option<bool>,
    prev_stoch: Option<StochState>,
    armed_side: Option<Side>,
    was_open: bool,
    cooldown_until: Option<u64>,
}

impl ElderTripleScreen {
    pub fn init() -> Self {
        let inds = Self::required_indicators_static();
        Self {
            ema_200_h4: inds[0],
            adx_14_h4: inds[1],
            rsi_14_h1: inds[2],
            stoch_rsi_h1: inds[3],
            ema_cross_m5: inds[4],
            atr_m5: inds[5],
            vol_ma_m5: inds[6],
            prev_ema_trend: None,
            prev_stoch: None,
            armed_side: None,
            was_open: false,
            cooldown_until: None,
        }
    }

    fn trend_bias(&self, indicators: &ValuesMap, last_price: &Price) -> Option<Side> {
        if last_price.close <= 0.0 {
            return None;
        }

        let ema_200 = match indicators.get(&self.ema_200_h4)?.value {
            EmaValue(v) => v,
            _ => return None,
        };

        let adx_14 = match indicators.get(&self.adx_14_h4)?.value {
            AdxValue(v) => v,
            _ => return None,
        };

        if adx_14 <= ADX_TREND_MIN {
            return None;
        }

        if last_price.close > ema_200 {
            Some(Side::Long)
        } else if last_price.close < ema_200 {
            Some(Side::Short)
        } else {
            None
        }
    }

    fn trend_invalid(&self, indicators: &ValuesMap, last_price: &Price, side: Side) -> bool {
        if last_price.close <= 0.0 {
            return false;
        }

        let ema_200 = match indicators.get(&self.ema_200_h4) {
            Some(tv) => match tv.value {
                EmaValue(v) => v,
                _ => return false,
            },
            None => return false,
        };

        let adx_14 = match indicators.get(&self.adx_14_h4) {
            Some(tv) => match tv.value {
                AdxValue(v) => v,
                _ => return false,
            },
            None => return false,
        };

        if adx_14 < ADX_TREND_EXIT {
            return true;
        }

        match side {
            Side::Long => last_price.close < ema_200,
            Side::Short => last_price.close > ema_200,
        }
    }

    fn update_stoch_cross(&mut self, indicators: &ValuesMap) -> Option<CrossDir> {
        let stoch_tv = indicators.get(&self.stoch_rsi_h1)?;
        if !stoch_tv.on_close {
            return None;
        }

        let (k, d) = match stoch_tv.value {
            StochRsiValue { k, d } => (k, d),
            _ => return None,
        };

        let curr = StochState { k, d };
        let prev = self.prev_stoch;
        self.prev_stoch = Some(curr);

        let prev = prev?;
        let cross_up = prev.k <= prev.d
            && curr.k > curr.d
            && prev.k < STOCH_LOW
            && prev.d < STOCH_LOW
            && curr.k > prev.k;
        let cross_down = prev.k >= prev.d
            && curr.k < curr.d
            && prev.k > STOCH_HIGH
            && prev.d > STOCH_HIGH
            && curr.k < prev.k;

        if cross_up {
            Some(CrossDir::Up)
        } else if cross_down {
            Some(CrossDir::Down)
        } else {
            None
        }
    }

    fn update_ema_cross(&mut self, indicators: &ValuesMap) -> Option<CrossDir> {
        let cross_tv = indicators.get(&self.ema_cross_m5)?;
        if !cross_tv.on_close {
            return None;
        }

        let trend = match cross_tv.value {
            EmaCrossValue { trend, .. } => trend,
            _ => return None,
        };

        let prev = self.prev_ema_trend;
        self.prev_ema_trend = Some(trend);

        match (prev, trend) {
            (Some(false), true) => Some(CrossDir::Up),
            (Some(true), false) => Some(CrossDir::Down),
            _ => None,
        }
    }
}

impl NeedsIndicators for ElderTripleScreen {
    fn required_indicators_static() -> Vec<IndexId> {
        vec![
            (IndicatorKind::Ema(200), Hour4),
            (
                IndicatorKind::Adx {
                    periods: 14,
                    di_length: 14,
                },
                Hour4,
            ),
            (IndicatorKind::Rsi(14), Hour1),
            (
                IndicatorKind::StochRsi {
                    periods: 14,
                    k_smoothing: Some(3),
                    d_smoothing: Some(3),
                },
                Hour1,
            ),
            (
                IndicatorKind::EmaCross { short: 9, long: 21 },
                Min5,
            ),
            (IndicatorKind::Atr(14), Min5),
            (IndicatorKind::VolMa(20), Min5),
        ]
    }
}

impl Strat for ElderTripleScreen {
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

        if armed.is_none() {
            self.armed_side = None;
        }

        if self.was_open {
            self.was_open = false;
            self.cooldown_until = Some(last_price.open_time + timedelta!(Hour1, 1).as_ms());
        }

        if let Some(until) = self.cooldown_until {
            if last_price.open_time < until {
                if armed.is_some() {
                    self.armed_side = None;
                    return Some(Intent::Disarm);
                }
                let _ = self.update_stoch_cross(indicators);
                let _ = self.update_ema_cross(indicators);
                return None;
            }
            self.cooldown_until = None;
        }

        let trend_bias = self.trend_bias(indicators, &last_price);
        let stoch_cross = self.update_stoch_cross(indicators);
        let ema_cross = self.update_ema_cross(indicators);

        if armed.is_some() {
            let expected_side = match self.armed_side {
                Some(side) => side,
                None => {
                    return Some(Intent::Disarm);
                }
            };

            if trend_bias != Some(expected_side) {
                self.armed_side = None;
                return Some(Intent::Disarm);
            }

            let cross_dir = ema_cross?;
            let atr = match indicators.get(&self.atr_m5)?.value {
                AtrValue(v) => v,
                _ => return None,
            };
            let vol_ma = match indicators.get(&self.vol_ma_m5)?.value {
                VolumeMaValue(v) => v,
                _ => return None,
            };

            if last_price.vlm <= vol_ma {
                return None;
            }

            let ref_px = last_price.close;
            if ref_px <= 0.0 || atr <= 0.0 {
                return None;
            }

            let sl_delta = (SL_ATR_MULT * atr / ref_px) * lev as f64;
            let tp_delta = (TP_ATR_MULT * atr / ref_px) * lev as f64;
            let tpsl = Some(Triggers {
                tp: Some(tp_delta),
                sl: Some(sl_delta),
            });

            let size = SizeSpec::MarginPct(ENTRY_MARGIN_PCT);
            let intent = match (expected_side, cross_dir) {
                (Side::Long, CrossDir::Up) => {
                    Some(Intent::open_market(Side::Long, size, tpsl))
                }
                (Side::Short, CrossDir::Down) => {
                    Some(Intent::open_market(Side::Short, size, tpsl))
                }
                _ => None,
            };

            if intent.is_some() {
                self.armed_side = None;
            }

            return intent;
        }

        let rsi_1h = match indicators.get(&self.rsi_14_h1)?.value {
            RsiValue(v) => v,
            _ => return None,
        };

        if let Some(trend) = trend_bias {
            if let Some(cross) = stoch_cross {
                match (trend, cross) {
                    (Side::Long, CrossDir::Up)
                        if rsi_1h >= RSI_LONG_MIN && rsi_1h <= RSI_LONG_MAX =>
                    {
                        self.armed_side = Some(Side::Long);
                        return Some(Intent::Arm(timedelta!(Hour1, 1)));
                    }
                    (Side::Short, CrossDir::Down)
                        if rsi_1h >= RSI_SHORT_MIN && rsi_1h <= RSI_SHORT_MAX =>
                    {
                        self.armed_side = Some(Side::Short);
                        return Some(Intent::Arm(timedelta!(Hour1, 1)));
                    }
                    _ => {}
                }
            }
        }

        None
    }

    fn on_open(&mut self, ctx: StratContext, open_pos: &OpenPosInfo) -> Option<Intent> {
        let StratContext {
            last_price,
            indicators,
            ..
        } = ctx;

        self.was_open = true;
        self.armed_side = None;
        let _ = self.update_stoch_cross(indicators);
        let _ = self.update_ema_cross(indicators);

        if self.trend_invalid(indicators, &last_price, open_pos.side) {
            return Some(Intent::flatten_market());
        }

        None
    }

    fn on_busy(&mut self, _ctx: StratContext, _busy: BusyType) -> Option<Intent> {
        None
    }
}
