use hyperliquid_rust_sdk::{ExchangeClient};
use log::info;
use std::fmt;

#[derive(Clone, Debug, Copy)]
pub enum Risk {
    Low,
    Medium,
    High,
}

#[derive(Clone, Debug, Copy)]
pub enum Strategy {
    Bull,
    Bear,
    Neutral,
}



#[derive(Clone, Debug)]
pub struct TradeParams {
    pub strategy: Strategy,
    pub risk: Risk,
    pub lev: u32,
    pub trade_time: u64, // in seconds
    pub asset: String,
    pub time_frame: String,
}



impl TradeParams{

    pub async fn update_lev(&mut self, lev: u32, client: &ExchangeClient){    
        
            let response = client
            .update_leverage(lev, self.asset.as_str() , false, None)
            .await
            .unwrap();
        
            info!("Update leverage response: {response:?}");
    }


}







impl Default for TradeParams {
    fn default() -> Self {
        Self {
            strategy: Strategy::Neutral,
            risk: Risk::Medium,
            lev: 20,
            trade_time: 300,
            asset: String::from("SOL"),
            time_frame: String::from("5m"),
        }
    }
}

impl fmt::Display for TradeParams {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "leverage: {}\nStrategy: {:?}\nRisk: {:?},\nTrade time: {} s\nasset: {}\ntime_frame: {}",
            self.lev,
            self.strategy,
            self.risk,
            self.trade_time,
            self.asset,
            self.time_frame
        )
    }
}
