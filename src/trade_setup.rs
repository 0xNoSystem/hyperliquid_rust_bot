use hyperliquid_rust_sdk::{ExchangeClient};
//use kwant::indicators::{Rsi, StochRsi, Atr, Adx, Ema, EmaCross, Sma};
use log::info;
use std::fmt;




#[derive(Clone, Debug, Copy)]
pub enum Risk {
    Low,
    Medium,
    High,
}

#[derive(Clone, Debug, Copy)]
pub enum Style{
    Scalp,
    Swing,
}

#[derive(Clone, Debug, Copy)]
pub enum Direction{
    Bull,
    Bear,
    Neutral,
}


#[derive(Clone, Debug, Copy)]
pub struct Strategy {
   pub risk: Risk,
   pub style: Style,    
   pub direction: Direction,
   pub follow_trend: bool,
}







impl Strategy{

    pub fn new(risk: Risk, style: Style, trend: Direction, follow_trend: bool) -> Self{
        Self { risk, style, trend, follow_trend }
    }

    
    pub fn get_rsi_threshold(&self) -> (f32, f32){
        match self.risk{
            Risk::Low => (25.0, 78.0),
            Risk::Medium => (30.0, 70.0),
            Risk::High => (33.0, 68.0),
        }
    }

    pub fn get_atr_threshold(&self) -> f32{
        match self.risk{
            Risk::Low => 0.001,
            Risk::Medium => 0.002,
            Risk::High => 0.003,
        }
    }




    pub fn update_risk(&mut self, risk: Risk){
        self.risk = risk;
    }

    pub fn update_style(&mut self, style: Style){
        self.style = style;
    }

    pub fn update_direction(&mut self, direction: Direction){
        self.direction = direction;
    }
    
    pub fn update_follow_trend(&mut self, follow_trend: bool){
        self.follow_trend = follow_trend;
    }
}


impl Default for Strategy{
    fn default() -> Self {
        Self { risk: Risk::Medium, style: Style::Scalp, direction: Direction::Neutral, follow_trend: true }
    }
}









#[derive(Clone, Debug)]
pub struct TradeParams {
    pub strategy: Strategy, 
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
