#![allow(unused_variables)]
use super::*;
use TimeFrame::*;
use Value::*;

pub struct RsiEmaScalp {
    rsi_1h: IndexId,
    rsi_15m: IndexId,
    ema_cross_15m: IndexId,
    prev_fast_above: Option<bool>,
}

impl RsiEmaScalp {
    pub fn init() -> Self {
        let inds = Self::required_indicators_static();
        RsiEmaScalp {
            rsi_1h: inds[0],
            ema_cross_15m: inds[1],
            rsi_15m: inds[2],
            prev_fast_above: None,
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

    fn on_idle(&mut self, ctx: StratContext, armed: Armed) -> Option<Intent>{
         let StratContext {
            free_margin,
            lev,
            last_price,
            indicators,
        } = ctx;
        
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

        if let Some(_expiry) = armed && let Some(prev_uptrend) = self.prev_fast_above.take(){
            if  !prev_uptrend && uptrend{
                return Some(Intent::open_market(Side::Long, SizeSpec::MarginPct(50.0), None));
            }
        }else if rsi_1h_value < 30.0 && !uptrend{
            return Some(Intent::Arm(timedelta!(Min15, 1)));
        }
        self.prev_fast_above = Some(uptrend);
        None
    }

    fn on_open(&mut self, ctx: StratContext, open_pos: &OpenPosInfo) -> Option<Intent>{
        let StratContext {
            free_margin,
            lev,
            last_price,
            indicators,
        } = ctx;
       
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


        if rsi_15m_value >= 50.0|| ((last_price.open_time - open_pos.open_time > timedelta!(Min15, 1).as_ms()) && rsi_1h_value < 35.0){
            let ttl = TimeoutInfo{
                action: OnTimeout::Force,
                duration: timedelta!(Min15, 1),
            };
            let order = Intent::flatten_limit(last_price.close * 1.003, Some(ttl));
            return Some(order);
        }

        self.prev_fast_above = Some(uptrend);
        None
    }

    fn on_busy(&mut self, ctx: StratContext, busy: BusyType) -> Option<Intent>{
        //ONLY ABORT IF NEEDED
        None
    }

    
}
