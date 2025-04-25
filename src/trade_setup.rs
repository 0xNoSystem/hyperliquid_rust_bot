use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

use log::info;
use serde::Deserialize;
use hyperliquid_rust_sdk::{ExchangeClient, ExchangeResponseStatus};
use kwant::indicators::Price;

use crate::strategy::{Strategy, CustomStrategy};



#[derive(Clone, Debug)]
pub struct TradeParams {
    pub strategy: Strategy, 
    pub lev: u32,
    pub trade_time: u64,  
    pub time_frame: TimeFrame,
}



impl TradeParams{

    pub async fn update_lev(&mut self, lev: u32, client: &ExchangeClient, asset: &str) -> Option<u32>{   
            
            let response = client
            .update_leverage(lev, asset, false, None)
            .await.unwrap();

            info!("Update leverage response: {response:?}");
            match response{
                ExchangeResponseStatus::Ok(_) => {
                    if self.lev == lev{
                        return None;
                    }else{
                        self.lev = lev;
                        return Some(lev);
                    }
            },
                ExchangeResponseStatus::Err(_)=>{
                    return None;
            },
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


#[derive(Clone, Debug, Copy)]
pub enum TradeCommand{
    ExecuteTrade {size: f32, is_long: bool, duration: u64},
    OpenTrade {size: f32, is_long: bool},
    CloseTrade{size: f32},
    BuildPosition {size: f32, is_long: bool, interval: u64},
    CancelTrade,
    Pause,
}

#[derive(Clone, Debug, Copy)]
pub struct PriceData{
    pub price: Price,
    pub time: u64,
}

#[derive(Clone, Debug, Copy)]
pub struct TradeInfo{
    pub open: f32,
    pub close: f32,
    pub pnl: f32,
    pub fee: f32,
    pub is_long: bool,
    pub duration: Option<u64>,
    pub oid: (u64, u64),
}




#[derive(Clone, Debug)]
pub struct TradeFillInfo{
    pub price: f32,
    pub fill_type: String,
    pub sz: f32,
    pub oid: u64,  
    pub is_long: bool,
}




//TIME FRAME
#[derive(Debug, Clone, Copy, PartialEq, Eq,Deserialize, Hash)]
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




impl TimeFrame{
    
    pub fn to_secs(&self) -> u64{
        match *self {
            TimeFrame::Min1   => 1 * 60,
            TimeFrame::Min3   => 3 * 60,
            TimeFrame::Min5   => 5 * 60,
            TimeFrame::Min15  => 15 * 60,
            TimeFrame::Min30  => 30 * 60,
            TimeFrame::Hour1  => 1 * 60 * 60,
            TimeFrame::Hour2  => 2 * 60 * 60,
            TimeFrame::Hour4  => 4 * 60 * 60,
            TimeFrame::Hour12 => 12 * 60 * 60,
            TimeFrame::Day1   => 24 * 60 * 60,
            TimeFrame::Day3   => 3 * 24 * 60 * 60,
            TimeFrame::Week   => 7 * 24 * 60 * 60,
            TimeFrame::Month  => 30 * 24 * 60 * 60, // approximate month as 30 days
        }
    }

    pub fn to_millis(&self) -> u64{
        self.to_secs() * 1000
    }


}

impl TimeFrame {
    pub fn as_str(&self) -> &'static str {
        match self {
            TimeFrame::Min1   => "1m",
            TimeFrame::Min3   => "3m",
            TimeFrame::Min5   => "5m",
            TimeFrame::Min15  => "15m",
            TimeFrame::Min30  => "30m",
            TimeFrame::Hour1  => "1h",
            TimeFrame::Hour2  => "2h",
            TimeFrame::Hour4  => "4h",
            TimeFrame::Hour12 => "12h",
            TimeFrame::Day1   => "1d",
            TimeFrame::Day3   => "3d",
            TimeFrame::Week   => "w",
            TimeFrame::Month  => "m",
        }
    }
    pub fn to_string(&self) -> String{
        
        self.as_str().to_string()

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
            "1m"  => Ok(TimeFrame::Min1),
            "3m"  => Ok(TimeFrame::Min3),
            "5m"  => Ok(TimeFrame::Min5),
            "15m" => Ok(TimeFrame::Min15),
            "30m" => Ok(TimeFrame::Min30),
            "1h"  => Ok(TimeFrame::Hour1),
            "2h"  => Ok(TimeFrame::Hour2),
            "4h"  => Ok(TimeFrame::Hour4),
            "12h" => Ok(TimeFrame::Hour12),
            "1d"  => Ok(TimeFrame::Day1),
            "3d"  => Ok(TimeFrame::Day3),
            "w"   => Ok(TimeFrame::Week),
            "m"   => Ok(TimeFrame::Month),
         _     => Err(format!("Invalid TimeFrame string: '{}'", s)),
        }
    }
}



