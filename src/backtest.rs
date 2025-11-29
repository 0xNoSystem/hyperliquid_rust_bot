use crate::{MARKETS, Error};  
use crate::helper::{load_candles};
use kwant::indicators::{Price};
use crate::trade_setup::{TradeParams, TimeFrame};
use crate::strategy::{Strategy, CustomStrategy, Style, Stance};
use crate::signal::{SignalEngine, ExecParams, IndexId};
use crate::helper::{get_asset};
use tokio::time::{sleep, Duration};
use hyperliquid_rust_sdk::{InfoClient, BaseUrl};

pub struct BackTester{
    info: InfoClient,
}



impl BackTester{

    pub fn new(client: InfoClient) -> Self{
        BackTester{
            info: client,
        }
    }

    pub fn run(asset: &str,params: ExecParams, strategy: Strategy, start: u64, end: u64) -> Result<(), Error>{
        if !MARKETS.contains(&asset){
            return Err(Error::BacktestError(format!("ASSET ({}) ISN'T TRADABLE", asset)));
        }
        if end >= start{
            return Err(Error::BacktestError(format!("Invalid time slice <start> should be less than <end>")));
        }
        
          


         
        Ok(()) 
        
    }
}
