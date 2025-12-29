#![allow(unused_variables)]
use super::*;
use TimeFrame::*;
use Value::*;

const RSI_THRESH: f64 = 28f64;
const ADX_THRESH: f64 = 35f64;
const NATR_THRESH: f64 = 0.03;

pub struct RsiChopSwing{
    rsi_1h: IndexId,
    adx_12h: IndexId,
    atr_1d: IndexId,
    active_window_start: Option<u64>,
    sl_set: bool,
    tp_set: bool,
    closing: bool,
}

impl RsiChopSwing{
    pub fn init() -> Self{
        let inds = Self::required_indicators_static();
        RsiChopSwing{
            rsi_1h: inds[0],
            adx_12h: inds[1],
            atr_1d: inds[2],
            active_window_start: None,
            sl_set: false,
            tp_set: false,
            closing: false,
        }
    }
}

impl NeedsIndicators for RsiChopSwing{
    fn required_indicators_static() -> Vec<IndexId>{
       vec!{
           (IndicatorKind::Rsi(12), Hour1),
           (IndicatorKind::Adx{periods: 14, di_length: 10}, Hour12),
           (IndicatorKind::Atr(14), Day1),
       }
    }
}

impl Strat for RsiChopSwing{
    fn required_indicators(&self) -> Vec<IndexId>{
        Self::required_indicators_static() 
    } 
    
    fn on_tick(&mut self, ctx: StratContext) -> Option<EngineOrder>{
        let StratContext {
            free_margin,
            lev,
            last_price,
            indicators,
            tick_time,
            open_pos,
        } = ctx;

        if self.closing{
            if open_pos.is_some(){
                return None;
            }else{
                self.closing = false;
            }
        }

        let max_size = (free_margin * lev as f64) / last_price; 

        let rsi_1h_value = match indicators.get(&self.rsi_1h)?.value {
            RsiValue(v) => v,
            _ => return None,
        };

        let atr_1d_value = match indicators.get(&self.atr_1d)?.value {
            SmaRsiValue(v) => v,
            _ => return None,
        };
        let atr_normalized = atr_1d_value / last_price;
        
        let adx_12h_value = match indicators.get(&self.adx_12h)?.value {
            AdxValue(v) => v,
            _ => return None,
        };

        (||{
            if let Some(pos) = open_pos{
                if !self.tp_set{
                    //todo
                }
                if !self.sl_set{
                    //todo
                }
            }

            if self.active_window_start.is_none() && (atr_normalized < NATR_THRESH || adx_12h_value > ADX_THRESH){
                self.active_window_start = Some(tick_time);
            }
            
            if let Some(start) = self.active_window_start{
                if tick_time - start > timedelta!(Hour1, 10){
                    self.active_window_start = None;
                    return None;
                }
                if rsi_1h_value < RSI_THRESH{
                    let side = Side::Long;
                    //todo
                }else if rsi_1h_value > 1.0 - RSI_THRESH{
                    let side = Side::Short;
                    //todo 
                }
            }

            None
            
        })()
    }

}


