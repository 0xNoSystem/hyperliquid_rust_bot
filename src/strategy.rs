#![allow(unused_variables)]
#![allow(unused_assignments)]

use crate::signal::ExecParams;
use crate::{
    ClientOrderLocal, EngineOrder, Limit, MAX_DECIMALS, PositionOp, Tif, TriggerKind, TriggerOrder,
    Value, roundf,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Copy, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum Risk {
    Low,
    Normal,
    High,
}

#[derive(Clone, Debug, Copy, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum Style {
    Scalp,
    Swing,
}

#[derive(Clone, Debug, Copy, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum Stance {
    Bull,
    Bear,
    Neutral,
}

#[derive(Clone, Debug, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Strategy {
    Custom(CustomStrategy),
}

#[derive(Clone, Debug, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomStrategy {
    pub risk: Risk,
    pub style: Style,
    pub stance: Stance,
    pub follow_trend: bool,
}

pub struct RsiRange {
    pub low: f64,
    pub high: f64,
}

pub struct AtrRange {
    pub low: f64,
    pub high: f64,
}

pub struct StochRange {
    pub low: f64,
    pub high: f64,
}

impl CustomStrategy {
    pub fn new(risk: Risk, style: Style, stance: Stance, follow_trend: bool) -> Self {
        Self {
            risk,
            style,
            stance,
            follow_trend,
        }
    }

    pub fn get_rsi_threshold(&self) -> RsiRange {
        match self.risk {
            Risk::Low => RsiRange {
                low: 25.0,
                high: 78.0,
            },
            Risk::Normal => RsiRange {
                low: 30.0,
                high: 70.0,
            },
            Risk::High => RsiRange {
                low: 33.0,
                high: 67.0,
            },
        }
    }

    pub fn get_stoch_threshold(&self) -> StochRange {
        match self.risk {
            Risk::Low => StochRange {
                low: 2.0,
                high: 95.0,
            },
            Risk::Normal => StochRange {
                low: 15.0,
                high: 85.0,
            },
            Risk::High => StochRange {
                low: 20.0,
                high: 80.0,
            },
        }
    }

    pub fn get_atr_threshold(&self) -> AtrRange {
        match self.risk {
            Risk::Low => AtrRange {
                low: 0.2,
                high: 1.0,
            },
            Risk::Normal => AtrRange {
                low: 0.5,
                high: 3.0,
            },
            Risk::High => AtrRange {
                low: 0.8,
                high: f64::INFINITY,
            },
        }
    }

    pub fn update_risk(&mut self, risk: Risk) {
        self.risk = risk;
    }

    pub fn update_style(&mut self, style: Style) {
        self.style = style;
    }

    pub fn update_direction(&mut self, stance: Stance) {
        self.stance = stance;
    }

    pub fn update_follow_trend(&mut self, follow_trend: bool) {
        self.follow_trend = follow_trend;
    }

    pub fn generate_test_trade(&self, price: f64, params: ExecParams) -> Option<EngineOrder> {

        let margin = if let Some(pos) = params.open_pos{
            params.margin - ((pos.entry_px * pos.size) / params.lev as f64)
        }else{
            params.margin
        };

        let px_tick = MAX_DECIMALS - params.sz_decimals - 1;
        let duration = 30;
        let is_long = true;

        let max_size = (margin * params.lev as f64) / price;
        if max_size * price < 10.0{
            return None;
        }

        let trigger = TriggerOrder {
            kind: TriggerKind::Sl,
            is_market: false,
        };
        let price = roundf!(price * 0.98, px_tick);
        let limit = Some(Limit::new_limit(price, Tif::Gtc));
        Some(EngineOrder {
            action: PositionOp::OpenLong,
            size: roundf!(max_size * 0.9, params.sz_decimals),
            limit: None,
        })
    }

    pub fn generate_test_tpsl(&self, price: f64, params: ExecParams) -> Option<EngineOrder> {
        let max_size = (params.margin * params.lev as f64) / price;

        let trigger = TriggerOrder {
            kind: TriggerKind::Sl,
            is_market: true,
        };

        None
    }

    pub fn generate_signal(
        &self,
        data: Vec<Value>,
        price: f64,
        params: ExecParams,
    ) -> Option<EngineOrder> {
        // Extract indicator values from the data
        let mut rsi_value = None;
        let mut srsi_value = None;
        let mut stoch_rsi = None;
        let mut ema_cross = None;
        let mut adx_value = None;
        let mut atr_value = None;

        for value in data {
            match value {
                Value::RsiValue(rsi) => rsi_value = Some(rsi),
                Value::SmaRsiValue(srsi) => srsi_value = Some(srsi),
                Value::StochRsiValue { k, d } => stoch_rsi = Some((k, d)),
                Value::EmaCrossValue { short, long, trend } => {
                    ema_cross = Some((short, long, trend))
                }
                Value::AdxValue(adx) => adx_value = Some(adx),
                Value::AtrValue(atr) => atr_value = Some(atr),
                _ => {} // Handle other indicators as needed
            }
        }

        //self.standard_strategy(rsi_value, stoch_rsi, ema_cross, adx_value, atr_value, price)
        if let Some(rsi) = rsi_value
            && let Some(srsi) = srsi_value
            && let Some(stoch) = stoch_rsi
        {
            let max_size = (params.margin * params.lev as f64) / price;
            return None;
        }

        None
    }
    /*
    fn rsi_based_scalp(
        &self,
        rsi: f64,
        srsi: f64,
        stoch_rsi: (f64, f64), // (K, D)
        max_size: f64,
    ) -> Option<EngineOrder> {
        let (k, d) = stoch_rsi;
        let duration = 420;

        let rsi_dev = match self.risk {
            Risk::Low => 15.0,
            Risk::Normal => 30.0,
            Risk::High => 37.0,
        };

        const SRSI_OB: f64 = 80.0;
        const SRSI_OS: f64 = 20.0;

        if self.stance != Stance::Bull {
            let rsi_short = rsi > 100.0 - rsi_dev;
            let srsi_short = srsi > 100.0 - rsi_dev - 5.0;
            let stoch_short = k > SRSI_OB && d > SRSI_OB;

            if rsi_short && srsi_short && stoch_short {
                return Some(ExecCommand::ExecuteTrade {
                    size: 0.9 * max_size,
                    is_long: false,
                    duration,
                });
            }
        }

        if self.stance != Stance::Bear {
            let rsi_long = rsi < rsi_dev;
            let srsi_long = srsi < rsi_dev + 5.0;
            let stoch_long = k < SRSI_OS && d < SRSI_OS;

            if rsi_long && srsi_long && stoch_long {
                return Some(ExecCommand::ExecuteTrade {
                    size: 0.9 * max_size,
                    is_long: true,
                    duration,
                });
            }
        }

        None
    }
    */
}

impl Default for CustomStrategy {
    fn default() -> Self {
        Self {
            risk: Risk::Normal,
            style: Style::Scalp,
            stance: Stance::Neutral,
            follow_trend: true,
        }
    }
}
