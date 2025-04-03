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
pub enum Stance{
    Bull,
    Bear,
    Neutral,
}


#[derive(Clone, Debug, Copy)]
pub struct Strategy {
   pub risk: Risk,
   pub style: Style,    
   pub stance: Stance,
   pub follow_trend: bool,
}

pub struct RsiRange{
    pub low: f32,
    pub high: f32,
}

pub struct AtrRange{
    pub low: f32,
    pub high: f32,
}

pub struct StochRange{
    pub low: f32,
    pub high: f32,
}


impl Strategy{

    pub fn new(risk: Risk, style: Style, stance: Stance, follow_trend: bool) -> Self{
        Self { risk, style, stance, follow_trend }
    }

    
    pub fn get_rsi_threshold(&self) -> RsiRange{
        match self.risk{
            Risk::Low => RsiRange{low: 25.0, high: 78.0},
            Risk::Medium => RsiRange{low: 30.0, high: 70.0},
            Risk::High => RsiRange{low: 33.0, high: 68.0},
        }
    }

    pub fn get_stoch_threshold(&self) -> StochRange{
        match self.risk{
            Risk::Low => StochRange{low: 2.0, high: 95.0},
            Risk::Medium => StochRange{low: 15.0, high: 85.0},
            Risk::High => StochRange{low:20.0, high: 80.0},
        }
    }


    pub fn get_atr_threshold(&self) -> AtrRange{
        match self.risk{
            Risk::Low => AtrRange{low: 0.2, high: 1.0},
            Risk::Medium => AtrRange{low: 0.5, high: 3.0},
            Risk::High => AtrRange{low: 0.8, high: f32::INFINITY},
        }
    }

    

    pub fn update_risk(&mut self, risk: Risk){
        self.risk = risk;
    }

    pub fn update_style(&mut self, style: Style){
        self.style = style;
    }

    pub fn update_direction(&mut self, stance: Stance){
        self.stance = stance;
    }
    
    pub fn update_follow_trend(&mut self, follow_trend: bool){
        self.follow_trend = follow_trend;
    }
}


impl Default for Strategy{
    fn default() -> Self {
        Self { risk: Risk::Medium, style: Style::Scalp, stance: Stance::Neutral, follow_trend: true }
    }
}









#[derive(Clone, Debug)]
pub struct TradeParams {
    pub strategy: Strategy, 
    pub lev: u32,
    pub trade_time: u64,  
    pub time_frame: String,
}



impl TradeParams{

    pub async fn update_lev(&mut self, lev: u32, client: &ExchangeClient, asset: String){    
        
            let response = client
            .update_leverage(lev, asset.as_str() , false, None)
            .await
            .unwrap();
        
            info!("Update leverage response: {response:?}");
    }


}



impl Default for TradeParams {
    fn default() -> Self {
        Self {
            strategy: Strategy::default(),
            lev: 20,
            trade_time: 300,
            time_frame: String::from("5m"),
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
            self.time_frame
        )
    }
}



pub struct TradeCommand{
    size: f32,
    is_long: bool,
}