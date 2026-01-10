use crate::HLTradeInfo;
use hyperliquid_rust_sdk::{
    ClientLimit, ClientOrder, ClientOrderRequest, ClientTrigger, Error, MarketOrderParams,
};
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::{OpenPosInfo, get_time_now, roundf};

#[derive(Clone, Debug)]
pub enum ExecCommand {
    Order(EngineOrder),
    Control(ExecControl),
    Event(ExecEvent),
}

#[derive(Copy, Clone, Debug)]
pub struct EngineOrder {
    pub action: PositionOp,
    pub size: f64,
    pub limit: Option<Limit>,
}

impl EngineOrder {
    pub fn new_market(action: PositionOp, size: f64) -> Self {
        EngineOrder {
            action,
            size,
            limit: None,
        }
    }

    pub fn new_market_open(side: Side, size: f64) -> Self {
        if side == Side::Long {
            Self::market_open_long(size)
        } else {
            Self::market_open_short(size)
        }
    }

    pub fn market_open_long(size: f64) -> Self {
        Self::new_market(PositionOp::OpenLong, size)
    }

    pub fn market_open_short(size: f64) -> Self {
        Self::new_market(PositionOp::OpenShort, size)
    }

    pub fn market_close(size: f64) -> Self {
        Self::new_market(PositionOp::Close, size)
    }

    //default limit order is a limit "Gtc" order non-trigger
    fn new_limit(
        action: PositionOp,
        size: f64,
        limit_px: f64,
        order_type: Option<ClientOrderLocal>,
    ) -> Self {
        let order_type = order_type.unwrap_or(ClientOrderLocal::ClientLimit(Tif::default()));

        EngineOrder {
            action,
            size,
            limit: Some(Limit::new(limit_px, order_type)),
        }
    }

    pub fn new_limit_close(size: f64, limit_px: f64, tif: Option<Tif>) -> Self {
        let order_type = ClientOrderLocal::ClientLimit(tif.unwrap_or_default());

        Self::new_limit(PositionOp::Close, size, limit_px, Some(order_type))
    }

    pub fn new_trigger_close(trigger_kind: TriggerKind, size: f64, trigger_px: f64) -> Self {
        let is_market = trigger_kind != TriggerKind::Tp;
        let order_type = ClientOrderLocal::ClientTrigger(TriggerOrder {
            kind: trigger_kind,
            is_market,
        });

        Self::new_limit(PositionOp::Close, size, trigger_px, Some(order_type))
    }

    pub fn new_limit_open(side: Side, size: f64, limit_px: f64, tif: Option<Tif>) -> Self {
        let order_type = ClientOrderLocal::ClientLimit(tif.unwrap_or_default());
        let action = if side == Side::Long {
            PositionOp::OpenLong
        } else {
            PositionOp::OpenShort
        };

        Self::new_limit(action, size, limit_px, Some(order_type))
    }

    pub fn limit_open_long(size: f64, limit_px: f64, tif: Option<Tif>) -> Self {
        Self::new_limit_open(Side::Long, size, limit_px, tif)
    }

    pub fn limit_open_short(size: f64, limit_px: f64, tif: Option<Tif>) -> Self {
        Self::new_limit_open(Side::Short, size, limit_px, tif)
    }

    pub fn new_tp(size: f64, trigger_px: f64) -> Self {
        Self::new_trigger_close(TriggerKind::Tp, size, trigger_px)
    }

    pub fn new_sl(size: f64, trigger_px: f64) -> Self {
        Self::new_trigger_close(TriggerKind::Sl, size, trigger_px)
    }

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
    pub fn new(limit_px: f64, order_type: ClientOrderLocal) -> Self {
        Limit {
            limit_px,
            order_type,
        }
    }
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
    ForceClose,
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

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum Tif {
    Alo,
    Ioc,
    #[default]
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

#[derive(Debug, Copy, Clone, Deserialize, Serialize)]
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

    pub fn apply_close_fill(
        &mut self,
        fill: &TradeFillInfo,
        sz_decimals: u32,
    ) -> Option<TradeInfo> {
        let close_sz = fill.sz;

        let price_diff = match self.side {
            Side::Long => fill.price - self.entry_px,
            Side::Short => self.entry_px - fill.price,
        };

        let partial_pnl = price_diff * close_sz;
        let net_chunk = partial_pnl - fill.fee;

        self.realised_pnl += net_chunk;
        self.size -= close_sz;
        self.fees += fill.fee;

        // still partially open
        if roundf!(self.size, sz_decimals) > 0.0 {
            return None;
        }

        //derive VWAP close price
        let gross_pnl = self.realised_pnl + self.fees;
        let avg_close_px = match self.side {
            Side::Long => self.entry_px + gross_pnl / close_sz,
            Side::Short => self.entry_px - gross_pnl / close_sz,
        };

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
                price: avg_close_px,
                fill_type: fill.fill_type,
            },
        })
    }

    pub fn sse(&self) -> OpenPosInfo {
        OpenPosInfo {
            side: self.side,
            size: self.size,
            entry_px: self.entry_px,
            open_time: self.open_time,
        }
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
