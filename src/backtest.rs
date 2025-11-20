use crate::{SignalEngine, MARKETS, IndexId};  
use crate::helper::{load_candles};
use kwant::indicators::{Price};
use crate::trade_setup::{TradeParams, TimeFrame};
use crate::strategy::{Strategy, CustomStrategy, Style, Stance};
use tokio::time::{sleep, Duration};
use hyperliquid_rust_sdk::{InfoClient, BaseUrl};

pub struct BackTester{
    pub asset: String,
    pub signal_engine: Option<SignalEngine>,
    pub params: TradeParams,
    pub candle_data: Vec<Price>,
    smallest_tf: TimeFrame,
}




impl BackTester{


    pub fn new_with_config(asset: &str,params: TradeParams, config: Option<Vec<IndexId>>, margin: f64) -> Self{
        if !MARKETS.contains(&asset){
            panic!("ASSET ISN'T TRADABLE, MARKET CAN'T BE INITILIAZED");
        }

        BackTester{
            asset: asset.to_string(),
            signal_engine: Some(SignalEngine::new_backtest(params.clone(), config, margin)),
            params,
            candle_data: Vec::new(),
            smallest_tf: TimeFrame::Min1,
        }
    }
}
