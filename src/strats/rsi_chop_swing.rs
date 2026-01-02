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
    opening: bool,
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
            opening: false,
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
            AtrValue(v) => v,
            _ => return None,
        };
        let atr_normalized = atr_1d_value / last_price;
        
        let adx_12h_value = match indicators.get(&self.adx_12h)?.value {
            AdxValue(v) => v,
            _ => return None,
        };
            if let Some(pos) = open_pos{
                if !self.tp_set{
                    self.tp_set = true;
                    let delta = if pos.side == Side::Long {1.03} else{ 0.97}; 
                    return Some(EngineOrder::new_tp(pos.size, pos.entry_px * delta));
                }
                if !self.sl_set{
                    self.sl_set = true;
                    let delta = if pos.side == Side::Long {0.98} else{ 1.02};
                    return Some(EngineOrder::new_sl(pos.size, pos.entry_px * delta));
                }

                if  pos.side == Side::Long && rsi_1h_value > 52.0{
                    self.closing = true;
                    return Some(EngineOrder::new_limit_close(pos.size, last_price * 1.001, None));
                }else if pos.side == Side::Short && rsi_1h_value < 48.0{
                    self.closing = true;
                    return Some(EngineOrder::new_limit_close(pos.size, last_price * 0.999, None)); 
                }
                self.opening = false;
                return None;
            }else{
                self.sl_set = false;
                self.tp_set = false;
            }

            if self.active_window_start.is_none() && (atr_normalized > NATR_THRESH && adx_12h_value < ADX_THRESH) && !self.opening{
                self.active_window_start = Some(tick_time);
            }
            
            if let Some(start) = self.active_window_start{
                if tick_time - start > timedelta!(Hour1, 10){
                    self.active_window_start = None;
                    return None;
                }
                if rsi_1h_value < RSI_THRESH{
                    let side = Side::Long;
                    let size = max_size * 0.9;

                    self.active_window_start = None;
                    self.opening = true;

                    return Some(EngineOrder::new_limit_open(
                        side,
                        size,
                        last_price * 0.997,
                        None,
                    ));
                }else if rsi_1h_value > 100.0 - RSI_THRESH{
                    let side = Side::Short;
                    let size = max_size * 0.9;

                    self.active_window_start = None;
                    self.opening = true;

                    return Some(EngineOrder::new_limit_open(
                        side,
                        size,
                        last_price * 1.003,
                        None,
                    ));
                }
            }
            None
    }

}


