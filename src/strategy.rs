#![allow(unused_variables)]
#![allow(unused_assignments)]

use crate::signal::{ExecParams, ValuesMap};
use crate::{
    ClientOrderLocal, EngineOrder, IndexId, IndicatorKind, Limit, MAX_DECIMALS, MIN_ORDER_VALUE,
    PositionOp, Tif, TimeFrame, TriggerKind, TriggerOrder, Value, get_time_now, roundf, timedelta,
};
use TimeFrame::*;
use Value::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Strategy {
    RsiEmaScalp,
}

impl Strategy {
    pub fn indicators(&self) -> Vec<IndexId> {
        use Strategy::*;
        match self {
            RsiEmaScalp => RsiEmaStrategy::required_indicators_static(),
        }
    }
}

#[derive(Clone, Debug, Copy, PartialEq, Deserialize, Serialize)]
pub struct RsiEmaStrategy {
    rsi_1h: IndexId,
    rsi_5m: IndexId,
    ema_cross_15m: IndexId,
    active_window_start: Option<u64>, //ms
    waiting_for_cross: bool,
    prev_fast_above: Option<bool>,
    tpsl_set: bool,
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
            tpsl_set: false,
        }
    }

    pub fn required_indicators_static() -> Vec<IndexId> {
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
        let sz_decimals = params.sz_decimals;
        let px_decimals = MAX_DECIMALS - sz_decimals - 1;
        let open_pos = params.open_pos;

        let max_size = roundf!((margin * lev) / price, sz_decimals);
        if max_size * price < MIN_ORDER_VALUE {
            return None;
        }

        let rsi_1h_value = match snapshot.get(&self.rsi_1h)? {
            RsiValue(v) => *v,
            _ => return None,
        };

        
        let rsi_5m_value = match snapshot.get(&self.rsi_5m)? {
            RsiValue(v) => *v,
            _ => return None,
        };

        let (fast, _slow, uptrend) = match snapshot.get(&self.ema_cross_15m)? {
            EmaCrossValue { short, long, trend } => (*short, *long, *trend),
            _ => return None,
        };
        let order = (|| {
            if let Some(open) = open_pos{
            if !self.tpsl_set
                && (rsi_5m_value >= 50.0
                || ((now - open.open_time > timedelta!(Min15, 1)) && rsi_1h_value < 35.0)
                || (price >= fast)
                    )
            {
                self.active_window_start = None;
                self.tpsl_set = true;
                return Some(EngineOrder {
                    action: PositionOp::Close,
                    size: roundf!(open.size, sz_decimals),
                    limit: Some(Limit {
                        limit_px: roundf!(price * 1.003, px_decimals),
                        order_type: ClientOrderLocal::ClientLimit(Tif::Gtc),
                    }),
                });
            }
            }else{
                self.tpsl_set = false;
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
                self.active_window_start = None;
                return Some(EngineOrder {
                    action: PositionOp::OpenLong,
                    size: roundf!(max_size * 0.9, sz_decimals),
                    limit: None,
                });
            }

            let open = open_pos?;
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

pub trait Strat {
    fn on_tick(
        &mut self,
        snapshot: ValuesMap,
        price: f64,
        params: &ExecParams,
        now: u64,
    ) -> Option<EngineOrder>;
    fn required_indicators(&self) -> Vec<IndexId>;
}
