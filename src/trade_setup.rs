use hyperliquid_rust_sdk::{Error, ExchangeClient, ExchangeResponseStatus};
use log::info;
use std::fmt;

use crate::strategy::Strategy;
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
