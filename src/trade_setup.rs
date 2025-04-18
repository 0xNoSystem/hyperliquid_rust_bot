use hyperliquid_rust_sdk::{ExchangeClient};
use std::collections::{HashMap, HashSet};
//use kwant::indicators::{Rsi, StochRsi, Atr, Adx, Ema, EmaCross, Sma};
use log::info;
use std::fmt;
use kwant::indicators::Price;
use serde::Deserialize;
use crate::helper::TimeFrame;

#[derive(Clone, Debug, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum Risk {
    Low,
    Normal,
    High,
}

#[derive(Clone, Debug, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum Style{
    Scalp,
    Swing,
}

#[derive(Clone, Debug, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum Stance{
    Bull,
    Bear,
    Neutral,
}


#[derive(Clone, Debug, Copy, PartialEq, Deserialize)]
pub struct Strategy {
   pub risk: Risk,
   pub style: Style,    
   pub stance: Stance,
   pub follow_trend: bool,
   pub index_strat: IndexConfig,
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

    pub fn new(risk: Risk, style: Style, stance: Stance, follow_trend: bool, index_strat: IndexConfig) -> Self{
        Self { risk, style, stance, follow_trend, index_strat }
    }

    
    pub fn get_rsi_threshold(&self) -> RsiRange{
        match self.risk{
            Risk::Low => RsiRange{low: 25.0, high: 78.0},
            Risk::Normal => RsiRange{low: 30.0, high: 70.0},
            Risk::High => RsiRange{low: 33.0, high: 67.0},
        }
    }

    pub fn get_stoch_threshold(&self) -> StochRange{
        match self.risk{
            Risk::Low => StochRange{low: 2.0, high: 95.0},
            Risk::Normal => StochRange{low: 15.0, high: 85.0},
            Risk::High => StochRange{low:20.0, high: 80.0},
        }
    }


    pub fn get_atr_threshold(&self) -> AtrRange{
        match self.risk{
            Risk::Low => AtrRange{low: 0.2, high: 1.0},
            Risk::Normal => AtrRange{low: 0.5, high: 3.0},
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
    
    pub fn update_index_strat(&mut self, new_config: IndexConfig){
        if self.index_strat != new_config{
            self.index_strat = new_config
        }
    }

}


impl Default for Strategy{
    fn default() -> Self {
        Self { 
            risk: Risk::Normal,
            style: Style::Scalp,
            stance: Stance::Neutral,
            follow_trend: true,
            index_strat: IndexConfig::default() }
    }
}









#[derive(Clone, Debug)]
pub struct TradeParams {
    pub strategy: Strategy, 
    pub lev: u32,
    pub trade_time: u64,  
    pub time_frame: TimeFrame,
}



impl TradeParams{

    pub async fn update_lev(&mut self, lev: u32, client: &ExchangeClient, asset: &str){    
        
            let response = client
            .update_leverage(lev, asset, false, None)
            .await
            .unwrap();
        
            info!("Update leverage response: {response:?}");
    }

    pub fn get_tfs(&self) -> Vec<TimeFrame>{

        let mut tfs = self.strategy.index_strat.get_tfs(); 
        if !tfs.contains(&self.time_frame){
            tfs.push(self.time_frame);
        }
        tfs 
    }
}



impl Default for TradeParams {
    fn default() -> Self {
        Self {
            strategy: Strategy::default(),
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
    CloseTrade{size: f32, is_long: bool},
    BuildPosition {size: f32, is_long: bool, interval: u64},
    CancelTrade,
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


#[derive(Debug, Clone, Copy, PartialEq,Deserialize, Eq, Hash)]
pub enum IndexStrat{
    None,
    Auto,
    Manual(TimeFrame),
}


#[derive(Debug, Clone, Copy,Deserialize, PartialEq, Eq, Hash)]
enum IndexKind{
    Rsi,
    SmaOnRsi,
    StochRsi,
    Adx,
    Atr,
    Ema,
    EmaCross,
    Sma,
}

#[derive(Debug,Deserialize, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IndexConfig{
    rsi: IndexStrat,
    sma_on_rsi: IndexStrat,
    stoch_rsi: IndexStrat,
    adx: IndexStrat,
    atr: IndexStrat,
    ema: IndexStrat,
    ema_cross: IndexStrat,
    sma: IndexStrat, 
}



impl IndexConfig{

    pub fn as_map(&self) -> HashMap<IndexKind,IndexStrat>{
        use IndexKind::*;

        let mut map = HashMap::new();
        map.insert(Rsi, self.rsi);
        map.insert(SmaOnRsi, self.sma_on_rsi);
        map.insert(StochRsi, self.stoch_rsi);
        map.insert(Adx, self.adx);
        map.insert(Atr, self.atr); 
        map.insert(Ema, self.ema);
        map.insert(EmaCross, self.ema_cross);
        map.insert(Sma, self.sma);
        map
    }

    fn get_tfs(&self) -> Vec<TimeFrame>{

        let mut tf_set = HashSet::new();
            for (kind, strat) in self.as_map().iter(){
               if let IndexStrat::Manual(tf) = strat{
                    tf_set.insert(*tf);
            }
        }

        let vec: Vec<TimeFrame> = tf_set.into_iter().collect();
        vec
}

}

impl Default for IndexConfig{

    fn default() -> Self{
        IndexConfig{
            rsi: IndexStrat::Auto,
            sma_on_rsi: IndexStrat::Auto,
            stoch_rsi: IndexStrat::Auto,
            adx: IndexStrat::Auto,
            atr: IndexStrat::Auto,
            ema: IndexStrat::Auto,
            ema_cross: IndexStrat::Auto,
            sma: IndexStrat::Auto,

        }        

    }
}
















