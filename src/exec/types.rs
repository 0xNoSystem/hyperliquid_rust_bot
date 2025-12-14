use crate::HLTradeInfo;
use hyperliquid_rust_sdk::{
    ClientLimit, ClientOrder, ClientOrderRequest, ClientTrigger, Error, MarketOrderParams,
};
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::{get_time_now, roundf};

#[derive(Clone, Copy, Debug)]
pub enum ExecCommand {
    Order(EngineOrder),
    Control(ExecControl),
    Event(ExecEvent),
}

#[derive(Clone, Copy, Debug)]
pub struct EngineOrder {
    pub action: PositionOp,
    pub size: f64,
    pub limit: Option<Limit>,
}

impl EngineOrder {
    pub fn is_tpsl(&self) -> Option<TriggerKind> {
        self.limit.map(|l| l.is_tpsl())?
    }
}

#[derive(Debug)]
pub enum HlOrder<'a> {
    Market(MarketOrderParams<'a>),
    Limit(ClientOrderRequest),
}

impl<'a> HlOrder<'a> {
    pub fn get_side(&self) -> Side {
        let is_long = match self {
            HlOrder::Market(order) => order.is_buy,
            HlOrder::Limit(order) => order.is_buy,
        };

        if is_long { Side::Long } else { Side::Short }
    }
    pub fn get_px(&self) -> Option<f64> {
        match self {
            HlOrder::Market(order) => order.px,
            HlOrder::Limit(order) => Some(order.limit_px),
        }
    }

    pub fn get_sz(&self) -> f64 {
        match self {
            HlOrder::Market(order) => order.sz,
            HlOrder::Limit(order) => order.sz,
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct Limit {
    pub limit_px: f64,
    pub order_type: ClientOrderLocal,
}

impl Limit {
    pub fn is_tpsl(&self) -> Option<TriggerKind> {
        match self.order_type {
            ClientOrderLocal::ClientLimit(_) => None,
            ClientOrderLocal::ClientTrigger(trigger) => Some(trigger.kind),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PositionOp {
    OpenLong,
    OpenShort,
    Close,
}

#[derive(Clone, Copy, Debug)]
pub enum ExecControl {
    Kill,
    Pause,
    Resume,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum ExecEvent {
    Fill(TradeFillInfo),
    Funding(f64),
}

#[derive(Clone, Copy, Debug)]
pub enum ClientOrderLocal {
    ClientLimit(Tif),
    ClientTrigger(TriggerOrder),
}

impl ClientOrderLocal {
    pub fn convert(&self, limit_px: f64) -> ClientOrder {
        match self {
            ClientOrderLocal::ClientLimit(tif) => ClientOrder::Limit(ClientLimit {
                tif: tif.to_string(),
            }),
            ClientOrderLocal::ClientTrigger(trigger) => ClientOrder::Trigger(ClientTrigger {
                is_market: trigger.is_market,
                trigger_px: limit_px,
                tpsl: trigger.kind.to_string(),
            }),
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct TriggerOrder {
    pub kind: TriggerKind,
    pub is_market: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum Tif {
    Alo,
    Ioc,
    Gtc,
}

impl fmt::Display for Tif {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Tif::Alo => "Alo",
            Tif::Ioc => "Ioc",
            Tif::Gtc => "Gtc",
        };
        write!(f, "{}", s)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum TriggerKind {
    Tp,
    Sl,
}

impl fmt::Display for TriggerKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            TriggerKind::Tp => "tp",
            TriggerKind::Sl => "sl",
        };
        write!(f, "{}", s)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TradeFillInfo {
    pub price: f64,
    pub sz: f64,
    pub oid: u64,
    pub fee: f64,
    pub side: Side,
    pub intent: PositionOp,
    pub fill_type: FillType,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Side {
    Long,
    Short,
}
impl std::ops::Not for Side {
    type Output = Side;

    fn not(self) -> Side {
        match self {
            Side::Long => Side::Short,
            Side::Short => Side::Long,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FillType {
    Market,
    Limit,
    Trigger(TriggerKind),
    Liquidation,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FillInfo {
    pub time: u64,
    pub price: f64,
    pub fill_type: FillType,
}

#[derive(Clone, Copy, Debug)]
pub struct FundingUpdate {
    funding: f64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TradeInfo {
    pub side: Side,
    pub size: f64,
    pub pnl: f64,
    pub fees: f64,
    pub funding: f64,
    pub open: FillInfo,
    pub close: FillInfo,
}

#[derive(Debug, Copy, Clone)]
pub enum OrderResponseLocal {
    Filled(TradeFillInfo),
    Resting(RestingOrderLocal),
}

#[derive(Debug, Copy, Clone)]
pub struct RestingOrderLocal {
    pub oid: u64,
    pub limit_px: Option<f64>,
    pub sz: f64,
    pub side: Side,
    pub intent: PositionOp,
    pub tpsl: Option<TriggerKind>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenPositionLocal {
    pub open_time: u64,
    pub size: f64,
    pub entry_px: f64,
    pub side: Side,
    pub fees: f64,
    pub funding: f64,
    pub realised_pnl: f64,
    pub fill_type: FillType,
}

impl OpenPositionLocal {
    pub fn new(fill: TradeFillInfo) -> Self {
        Self {
            open_time: get_time_now(),
            size: fill.sz,
            entry_px: fill.price,
            side: fill.side,
            realised_pnl: 0.0,
            fees: fill.fee,
            funding: 0.0,
            fill_type: fill.fill_type,
        }
    }

    pub fn apply_open_fill(&mut self, fill: &TradeFillInfo) {
        assert_eq!(self.side, fill.side);

        let old_size = self.size;
        let new_size = old_size + fill.sz;

        let new_entry_px = (self.entry_px * old_size + fill.price * fill.sz) / new_size;

        self.entry_px = new_entry_px;
        self.size = new_size;
        self.fees += fill.fee;
    }

    pub fn apply_close_fill(&mut self, fill: &TradeFillInfo, sz_decimals: u32) -> Option<TradeInfo> {
        let close_px = fill.price;
        let close_sz = fill.sz;

        let price_diff = match self.side {
            Side::Long => close_px - self.entry_px,
            Side::Short => self.entry_px - close_px,
        };

        let partial_pnl = price_diff * close_sz;
        let net_chunk = partial_pnl - fill.fee;

        self.realised_pnl += net_chunk;
        self.size -= close_sz;
        self.fees += fill.fee;

        if roundf!(self.size, sz_decimals) > 0.0 {
            return None;
        }

        Some(TradeInfo {
            side: self.side,
            size: close_sz,
            pnl: self.realised_pnl + self.funding,
            fees: self.fees,
            funding: self.funding,
            open: FillInfo {
                time: self.open_time,
                price: self.entry_px,
                fill_type: self.fill_type,
            },
            close: FillInfo {
                time: get_time_now(),
                price: close_px,
                fill_type: fill.fill_type,
            },
        })
    }
}

impl TryFrom<Vec<HLTradeInfo>> for TradeFillInfo {
    type Error = Error;

    fn try_from(fills: Vec<HLTradeInfo>) -> Result<Self, Self::Error> {
        if fills.is_empty() {
            return Err(Error::GenericParse(
                "TradeFillInfo::try_from called with empty fills vec".to_string(),
            ));
        }

        let first = &fills[0];

        // --- invariant checks ---
        for f in &fills {
            if f.oid != first.oid {
                return Err(Error::Custom(
                    "Mismatched oid in HLTradeInfo batch".to_string(),
                ));
            }
            if f.coin != first.coin {
                return Err(Error::Custom(
                    "Mismatched coin in HLTradeInfo batch".to_string(),
                ));
            }
            if f.side != first.side {
                return Err(Error::Custom(
                    "Mismatched side in HLTradeInfo batch".to_string(),
                ));
            }
            if f.dir != first.dir {
                return Err(Error::Custom(
                    "Mismatched dir in HLTradeInfo batch".to_string(),
                ));
            }
        }

        // --- side ---
        let side = match first.side.as_str() {
            "B" => Side::Long,
            "A" => Side::Short,
            other => {
                return Err(Error::GenericParse(format!(
                    "Unknown HL side value: {}",
                    other
                )));
            }
        };

        // --- intent (derived from dir) ---
        let intent = {
            let d = first.dir.as_str();
            if d.contains("Open") {
                if d.contains("Long") {
                    PositionOp::OpenLong
                } else if d.contains("Short") {
                    PositionOp::OpenShort
                } else {
                    return Err(Error::GenericParse(format!(
                        "Unknown Open direction in dir: {}",
                        d
                    )));
                }
            } else if d.contains("Close") {
                PositionOp::Close
            } else {
                return Err(Error::GenericParse(format!("Unknown dir value: {}", d)));
            }
        };

        // --- fill type (delta-based) ---
        let fill_type = if fills.iter().any(|f| f.liquidation.is_some()) {
            FillType::Liquidation
        } else if fills.iter().any(|f| f.crossed) {
            FillType::Market
        } else {
            FillType::Limit
        };

        // --- aggregate numerics ---
        let mut total_sz = 0.0;
        let mut weighted_px = 0.0;
        let mut total_fee = 0.0;

        for f in &fills {
            let sz: f64 = f.sz.parse().map_err(|_| Error::FloatStringParse)?;

            let px: f64 = f.px.parse().map_err(|_| Error::FloatStringParse)?;

            let fee: f64 = f.fee.parse().map_err(|_| Error::FloatStringParse)?;

            total_sz += sz;
            weighted_px += px * sz;
            total_fee += fee;
        }

        if total_sz <= 0.0 {
            return Err(Error::GenericParse(
                "Aggregated fill size is zero".to_string(),
            ));
        }

        Ok(TradeFillInfo {
            oid: first.oid,
            side,
            intent,
            price: weighted_px / total_sz,
            sz: total_sz,
            fee: total_fee,
            fill_type,
        })
    }
}
