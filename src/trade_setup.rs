use std::fmt;

use hyperliquid_rust_sdk::{Error, ExchangeClient, ExchangeResponseStatus, RestingOrder};
use log::info;
//use kwant::indicators::Price;

use crate::strategy::{CustomStrategy, Strategy};
use crate::{HLTradeInfo, get_time_now, roundf};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TradeParams {
    pub strategy: Strategy,
    pub lev: usize,
    pub trade_time: u64,
    pub time_frame: TimeFrame,
}

impl TradeParams {
    pub async fn update_lev(
        &mut self,
        lev: usize,
        client: &ExchangeClient,
        asset: &str,
        first_time: bool,
    ) -> Result<usize, Error> {
        if !first_time && self.lev == lev {
            return Err(Error::Custom("Leverage is unchanged".to_string()));
        }

        let response = client
            .update_leverage(lev as u32, asset, false, None)
            .await?;

        info!("Update leverage response: {response:?}");
        match response {
            ExchangeResponseStatus::Ok(_) => {
                self.lev = lev;
                Ok(lev)
            }
            ExchangeResponseStatus::Err(e) => Err(Error::Custom(e)),
        }
    }
}

impl Default for TradeParams {
    fn default() -> Self {
        Self {
            strategy: Strategy::Custom(CustomStrategy::default()),
            lev: 20,
            trade_time: 300,
            time_frame: TimeFrame::Min5,
        }
    }
}

impl fmt::Display for TradeParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "leverage: {}\nStrategy: {:?}\nTrade time: {} s\ntime_frame: {}",
            self.lev,
            self.strategy,
            self.trade_time,
            self.time_frame.as_str(),
        )
    }
}

#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TradeCommand {
    ExecuteTrade {
        size: f64,
        is_long: bool,
        duration: u64,
    },
    OpenTrade {
        size: f64,
        is_long: bool,
    },
    CloseTrade {
        size: f64,
    },
    LimitOpen(LimitOrderLocal),
    LimitClose {
        size: f64,
        limit_px: f64,
        tif: Tif,
    },
    Trigger(TriggerOrderLocal),
    BuildPosition {
        size: f64,
        is_long: bool,
        interval: u64,
    },
    //Canceling == Liq Taker
    CancelTrade,
    UserFills(TradeFillInfo),
    Toggle,
    Resume,
    Pause,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TradeInfo {
    pub open_px: f64,
    pub close_px: f64,
    pub is_long: bool,
    pub size: f64,
    pub pnl: f64,
    pub fees: f64,
    pub funding: f64,
    pub open_time: u64,
    pub close_time: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketTradeInfo {
    pub asset: String,
    pub info: TradeInfo,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TradeFillInfo {
    pub price: f64,
    pub fill_type: FillType,
    pub sz: f64,
    pub oid: u64,
    pub fee: f64,
    pub is_long: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FillType {
    MarketOpen,
    MarketClose,
    LimitOpen,
    LimitClose,
    Trigger(TriggerKind),
    Liquidation,
    Mixed,
}

impl From<&HLTradeInfo> for FillType {
    fn from(trade: &HLTradeInfo) -> Self {
        let dir = trade.dir.to_lowercase();
        if trade.crossed {
            if dir.contains("liquidation") {
                FillType::Liquidation
            } else if dir.contains("close") {
                FillType::MarketClose
            } else if dir.contains("open") {
                FillType::MarketOpen
            } else {
                FillType::Mixed
            }
        } else {
            if dir.contains("liquidation") {
                FillType::Liquidation
            } else if dir.contains("close") {
                FillType::LimitClose
            } else if dir.contains("open") {
                FillType::LimitOpen
            } else {
                FillType::Mixed
            }
        }
    }
}

impl FillType {
    fn is_close(&self) -> bool {
        use FillType::*;
        match self {
            Liquidation => true,
            MarketClose => true,
            LimitClose => true,
            Mixed => true,
            Trigger(_) => true,
            _ => false,
        }
    }
}

impl From<Vec<HLTradeInfo>> for TradeFillInfo {
    fn from(trades: Vec<HLTradeInfo>) -> Self {
        let oid = trades[0].oid;
        let is_long = match trades[0].side.as_str() {
            "A" => false,
            "B" => true,
            _ => panic!("TRADE SIDE IS NOT A NOR B, HYPERLIQUID API ERROR"),
        };

        let mut sz: f64 = f64::from_bits(1);
        let mut total: f64 = f64::from_bits(1);
        let mut fee: f64 = f64::from_bits(1);
        let mut fill_type: FillType = FillType::from(&trades[0]);

        trades.into_iter().for_each(|t| {
            if fill_type == FillType::Mixed && fill_type != FillType::from(&t) {
                fill_type = FillType::Mixed;
            }
            let size = t.sz.parse::<f64>().unwrap();
            total += size * t.px.parse::<f64>().unwrap();
            sz += size;
            fee += t.fee.parse::<f64>().unwrap();
        });

        let avg_px = total / sz;

        Self {
            price: avg_px,
            fee,
            fill_type,
            sz,
            oid,
            is_long,
        }
    }
}

impl TradeFillInfo {
    pub fn is_close(&self) -> bool {
        self.fill_type.is_close()
    }

    pub fn is_liquidation(&self) -> bool {
        self.fill_type == FillType::Liquidation
    }
}

//TIME FRAME
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Hash)]
#[serde(rename_all = "camelCase")]
pub enum TimeFrame {
    Min1,
    Min3,
    Min5,
    Min15,
    Min30,
    Hour1,
    Hour2,
    Hour4,
    Hour12,
    Day1,
    Day3,
    Week,
    Month,
}

impl TimeFrame {
    pub fn to_secs(&self) -> u64 {
        match *self {
            TimeFrame::Min1 => 60,
            TimeFrame::Min3 => 3 * 60,
            TimeFrame::Min5 => 5 * 60,
            TimeFrame::Min15 => 15 * 60,
            TimeFrame::Min30 => 30 * 60,
            TimeFrame::Hour1 => 60 * 60,
            TimeFrame::Hour2 => 2 * 60 * 60,
            TimeFrame::Hour4 => 4 * 60 * 60,
            TimeFrame::Hour12 => 12 * 60 * 60,
            TimeFrame::Day1 => 24 * 60 * 60,
            TimeFrame::Day3 => 3 * 24 * 60 * 60,
            TimeFrame::Week => 7 * 24 * 60 * 60,
            TimeFrame::Month => 30 * 24 * 60 * 60, // approximate month as 30 days
        }
    }

    pub fn to_millis(&self) -> u64 {
        self.to_secs() * 1000
    }
}

impl From<TimeFrame> for u8 {
    fn from(tf: TimeFrame) -> Self {
        tf.to_secs() as u8
    }
}
impl TimeFrame {
    pub fn as_str(&self) -> &'static str {
        match self {
            TimeFrame::Min1 => "1m",
            TimeFrame::Min3 => "3m",
            TimeFrame::Min5 => "5m",
            TimeFrame::Min15 => "15m",
            TimeFrame::Min30 => "30m",
            TimeFrame::Hour1 => "1h",
            TimeFrame::Hour2 => "2h",
            TimeFrame::Hour4 => "4h",
            TimeFrame::Hour12 => "12h",
            TimeFrame::Day1 => "1d",
            TimeFrame::Day3 => "3d",
            TimeFrame::Week => "1w",
            TimeFrame::Month => "1M",
        }
    }
}

impl std::fmt::Display for TimeFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for TimeFrame {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "1m" => Ok(TimeFrame::Min1),
            "3m" => Ok(TimeFrame::Min3),
            "5m" => Ok(TimeFrame::Min5),
            "15m" => Ok(TimeFrame::Min15),
            "30m" => Ok(TimeFrame::Min30),
            "1h" => Ok(TimeFrame::Hour1),
            "2h" => Ok(TimeFrame::Hour2),
            "4h" => Ok(TimeFrame::Hour4),
            "12h" => Ok(TimeFrame::Hour12),
            "1d" => Ok(TimeFrame::Day1),
            "3d" => Ok(TimeFrame::Day3),
            "1w" => Ok(TimeFrame::Week),
            "1M" => Ok(TimeFrame::Month),
            _ => Err(format!("Invalid TimeFrame string: '{}'", s)),
        }
    }
}

#[derive(Debug, Copy, Clone, Deserialize, Serialize, Hash)]
#[serde(rename_all = "camelCase")]
pub enum LiquiditySide {
    Maker,
    Taker,
}

#[derive(Debug, Copy, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitOrderLocal {
    pub size: f64,
    pub is_long: bool,
    pub limit_px: f64,
    pub tif: Tif,
}

#[derive(Debug, Copy, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerOrderLocal {
    pub size: f64,
    pub is_long: bool,
    pub trigger_px: f64,
    pub kind: TriggerKind,
    pub is_market: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenPositionLocal {
    pub asset: String,
    pub open_time: u64,
    pub size: f64,
    pub entry_px: f64,
    pub is_long: bool,
    pub fees: f64,
    pub funding: f64,
    pub realised_pnl: f64,
}

impl OpenPositionLocal {
    pub fn new(asset: String, fill: TradeFillInfo) -> Self {
        Self {
            asset,
            open_time: get_time_now(),
            size: fill.sz,
            entry_px: fill.price,
            is_long: fill.is_long,
            realised_pnl: 0.0,
            fees: fill.fee,
            funding: 0.0,
        }
    }

    pub fn apply_close_fill(&mut self, fill: &TradeFillInfo) -> Option<TradeInfo> {
        let close_px = fill.price;
        let close_sz = fill.sz;
        let close_fee = fill.fee;

        let price_diff = if self.is_long {
            close_px - self.entry_px
        } else {
            self.entry_px - close_px
        };

        let partial_trade_pnl = price_diff * close_sz;
        let chunk_realized = partial_trade_pnl - close_fee;

        self.realised_pnl += chunk_realized;
        self.size -= close_sz;
        self.fees += close_fee;

        if roundf!(self.size, 5) > 0.0 {
            return None;
        }

        Some(TradeInfo {
            open_px: self.entry_px,
            close_px,
            is_long: self.is_long,
            size: close_sz,
            pnl: self.realised_pnl + self.funding,
            fees: self.fees,
            funding: self.funding,
            open_time: self.open_time,
            close_time: get_time_now(),
        })
    }

    pub fn apply_open_fill(&mut self, fill: &TradeFillInfo) {
        let fill_px = fill.price;
        let fill_sz = fill.sz;
        let fill_fee = fill.fee;

        assert_eq!(self.is_long, fill.is_long);

        let old_size = self.size;
        let new_size = old_size + fill_sz;

        let new_entry_px = if roundf!(old_size, 6) == 0.0 {
            fill_px
        } else {
            (self.entry_px * old_size + fill_px * fill_sz) / new_size
        };

        self.entry_px = new_entry_px;
        self.size = new_size;
        self.fees += fill_fee;
    }
}

#[derive(Debug, Clone)]
pub enum LimitOrderResponseLocal {
    Filled(TradeFillInfo),
    Resting(RestingOrder),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Copy, Serialize, Deserialize)]
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

