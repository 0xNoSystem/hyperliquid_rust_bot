use hyperliquid_rust_sdk::{ExchangeClient};
use log::info;

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
            lev: 5,
            trade_time: 120,
            asset: String::from("SOL"),
            time_frame: String::from("5m"),
        }
    }
}


