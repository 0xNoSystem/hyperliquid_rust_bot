use log::info;
use kwant::indicators::{Rsi, Atr, Price, Indicator, Ema, EmaCross, Sma, Adx};
use crate::trade_setup::{PriceData, Strategy, TradeCommand, Style, Stance};
use crate::{MAX_HISTORY};
use tokio::sync::mpsc::UnboundedReceiver;
use flume::Sender;
 
use tokio::time::{sleep, Duration};

#[derive(Debug)]
pub struct SignalEngine{
    engine_rv: Option<UnboundedReceiver<EngineCommand>>,
    trade_tx: Option<Sender<TradeCommand>>,
    indicators_config: IndicatorsConfig,
    rsi: Rsi,
    atr: Atr,
    ema: Ema,
    ema_cross: Option<EmaCross>,
    adx: Adx,
    sma: Sma,
    strategy: Strategy,
    price_data: Vec<Price>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IndicatorsConfig{
    pub rsi_length: usize,
    pub rsi_smoothing: Option<usize>,
    pub stoch_rsi_length: usize,
    pub atr_length: usize,
    pub ema_length: usize,
    pub ema_cross_short_long_lenghts: Option<(usize, usize)>,
    pub adx_length: usize,
    pub sma_length: usize,
}


impl Default for IndicatorsConfig{

    fn default() -> Self {
        IndicatorsConfig{
            rsi_length: 14,
            rsi_smoothing: Some(10),
            stoch_rsi_length: 14,
            atr_length: 14,
            ema_length: 9,
            ema_cross_short_long_lenghts: Some((9, 21)),
            adx_length: 14,
            sma_length: 14,
        }
    }
}



impl SignalEngine{

    pub async fn new(config: Option<IndicatorsConfig>, strategy: Strategy) -> Self{
       
        let config: IndicatorsConfig = match config{
            Some(cfg) => cfg,
            None => IndicatorsConfig::default(),
        };

        let ema_cross = config.ema_cross_short_long_lenghts
        .map(|(short, long)| EmaCross::new(short, long));

        SignalEngine{
            engine_rv: None,
            trade_tx: None,
            indicators_config: config.clone(),
            rsi: Rsi::new(config.rsi_length, config.stoch_rsi_length ,config.rsi_smoothing),
            atr: Atr::new(config.atr_length),
            ema: Ema::new(config.ema_length),
            ema_cross: ema_cross,
            adx: Adx::new(config.adx_length, config.adx_length),
            sma: Sma::new(config.sma_length),
            strategy,
            price_data: Vec::new(),
        }
    }

    pub fn update(&mut self, price:Price, after_close: bool){
        
        if !after_close{
            self.update_before_close(price);
        }else{
            self.update_after_close(price);
        }
    }


    pub fn update_after_close(&mut self, price: Price){
        self.rsi.update_after_close(price);
        self.ema.update_after_close(price);
        if let Some(ref mut ema_cross) = self.ema_cross{
            ema_cross.update(price, true);
        }
        self.adx.update_after_close(price);
        self.atr.update_after_close(price);
        self.sma.update_after_close(price);
        if self.price_data.len() >= MAX_HISTORY{
            self.price_data.remove(0);
        }
        self.price_data.push(price);
    }

    pub fn update_before_close(&mut self, price: Price){
        self.rsi.update_before_close(price);
        self.ema.update_before_close(price);
        if let Some(ref mut ema_cross) = self.ema_cross{
            ema_cross.update(price, false);
        }
        self.adx.update_before_close(price);
        self.atr.update_before_close(price);
        self.sma.update_before_close(price);
    }

    pub fn reset(&mut self){
        self.rsi.reset();
        self.ema.reset();
        if let Some(ref mut ema_cross) = self.ema_cross{
            ema_cross.reset();
        }
        self.adx.reset();
        self.atr.reset();
        self.sma.reset();
    } 
    
    
     pub fn change_indicators_config(&mut self, config: IndicatorsConfig){
        if config != self.indicators_config{
            self.reset();
            let ema_cross = config.ema_cross_short_long_lenghts
            .map(|(short, long)| EmaCross::new(short, long));
            self.rsi = Rsi::new(config.rsi_length, config.stoch_rsi_length ,config.rsi_smoothing);
            self.atr = Atr::new(config.atr_length);
            self.ema = Ema::new(config.ema_length);
            self.ema_cross = ema_cross;
            self.adx= Adx::new(config.adx_length, config.adx_length);
            self.sma = Sma::new(config.sma_length);
            self.indicators_config = config.clone();
            self.load(&self.price_data.clone());
        }
     }



    pub fn get_indicators_config(&self) -> &IndicatorsConfig{
        &self.indicators_config
    }
    
    pub fn get_mut_strategy(&mut self) -> &mut Strategy{
        &mut self.strategy
    }

    pub fn change_strategy(&mut self, strategy: Strategy){
        self.strategy = strategy;
        println!("Strategy changed to: {:?}", self.strategy);
    }
    pub fn get_strategy(&self) -> &Strategy{
        &self.strategy
    }

    pub fn get_price_data(&self) -> &Vec<Price>{
        &self.price_data
    }	

     pub fn load(&mut self, price_data: &Vec<Price>){
        self.price_data.clear();
        for p in price_data{
            self.update_after_close(*p);
        }
    }

    pub fn is_ready(&self) -> bool{
        self.rsi.is_ready() && self.ema.is_ready() && self.atr.is_ready() && self.sma.is_ready() && (self.ema_cross.is_none() || self.ema_cross.clone().unwrap().is_ready())
    }

    pub fn get_rsi(&self) -> Option<f32>{
        self.rsi.get_last()
    }
    pub fn get_sma_rsi(&self) -> Option<f32>{
        self.rsi.get_sma_rsi()
    }
    pub fn get_stoch_rsi(&self) -> Option<f32>{
        self.rsi.get_stoch_rsi()
    }
    pub fn get_stoch_signal(&self) -> Option<f32>{
        self.rsi.get_stoch_signal()
    }
    pub fn get_ema(&self) -> Option<f32>{
        self.ema.get_last()
    }
    pub fn get_ema_slope(&self) -> Option<f32>{
        self.ema.get_slope()
    }
    pub fn get_ema_cross_trend(&self) -> Option<bool>{
        
        if let Some(ref ema_cross) = self.ema_cross{
            return ema_cross.get_trend()
        }else{
            return None;
        }
    }   

     pub fn check_ema_cross(&mut self) -> Option<bool>{
        if let Some(ref mut ema_cross) = self.ema_cross{
            ema_cross.check_for_cross()
        }else{
            None
        }
    }
    pub fn get_adx(&self) -> Option<f32>{
        self.adx.get_last()
    }
    pub fn get_atr(&self) -> Option<f32>{
        self.atr.get_last()
    }
    pub fn get_atr_normalized(&self, price: f32) -> Option<f32>{
        self.atr.normalized(price)
    }
    pub fn get_sma(&self) -> Option<f32>{
        self.sma.get_last()
    }
    
    pub fn get_last_price(&self) -> Option<Price>{
        self.price_data.last().cloned()
    }

    fn get_signal(&self) -> Option<TradeCommand>{

        if !self.is_ready(){
            return None;
        }

        let rsi_range = self.strategy.get_rsi_threshold();
        let stoch_range = self.strategy.get_stoch_threshold();
        

        let rsi = self.get_rsi().unwrap();
        let stoch = self.get_stoch_rsi().unwrap();
        
        match self.strategy.style{
            Style::Scalp => {
                if rsi < rsi_range.low && stoch < stoch_range.low{
                    if self.strategy.stance == Stance::Bear{
                        return None;
                    }else{
                        return Some(TradeCommand::ExecuteTrade{size: 2.0, is_long: true, duration: 240});
                    }
                }else if rsi > rsi_range.high && stoch > stoch_range.high{
                    if self.strategy.stance == Stance::Bull{
                        return None;
                    }else{
                        return Some(TradeCommand::ExecuteTrade{size: 2.0, is_long: false,duration: 240});
                    }
                }else{
                    return None;
                }
            },

            _ => {
                return None;
            }
        }
    }
}
impl SignalEngine{

    pub fn connect_market(
        &mut self,
        receiver: UnboundedReceiver<EngineCommand>,
        sender: Sender<TradeCommand>)
    {
        
        self.engine_rv = Some(receiver);
        self.trade_tx = Some(sender); 
    }

    pub fn is_connected(&self) -> bool{
        self.engine_rv.is_some() && self.trade_tx.is_some()
    }
    pub async fn start(&mut self){
        let mut time = 0;
        let mut init = false;
        if self.is_connected(){
            
        let tx_sender = self.trade_tx.clone().unwrap();
            while let Some(cmd) = self.engine_rv.as_mut().unwrap().recv().await{
           
            match cmd {

                EngineCommand::UpdatePrice(price_data) => {
                    let close_time = price_data.time;
                    let price = price_data.price.close;
                    println!("\n\nPRICE => {}$", price);
                    if !init{
                            self.update(price_data.price, false);
                            time = close_time;
                            init = true;
                            continue;
                        }


                    if time != close_time{
                            self.update(price_data.price, true); 
                            time = close_time;
                    }else{
                            self.update(price_data.price, false);
                    }

                        self.display_indicators(price);
                        
                        if let Some(trade_command) = self.get_signal(){
                            let _ = tx_sender.clone().try_send(trade_command);
                        }
            },

                    EngineCommand::UpdateStrategy(new_strat) =>{

                       self.change_strategy(new_strat);
                 },

                    EngineCommand::UpdateConfig(new_config) =>{
                        info!("NEW CONFIG: {:?}", new_config);
                        self.change_indicators_config(new_config);
                },

                    EngineCommand::Reload(prices)=>{
                        self.reset();
                        self.load(&prices);
                        info!("RELOADED ENGINE WITH NEW TIME FRAME DATA");
                    },
            }
        }}
    }

    fn display_indicators(&self, price: f32){

                    if let Some(stoch_rsi) = self.get_stoch_rsi(){
                println!("🔵STOCH-K: {}", stoch_rsi);
            }
            
            if let Some(stoch_rsi) = self.get_stoch_signal(){
                println!("🟠STOCH-D: {}", stoch_rsi);
            }

            if let Some(rsi_value) = self.get_rsi(){
                println!("🟢RSI: {}", &rsi_value);
                    
            };

            if let Some(rsi_value) = self.get_sma_rsi(){
                println!("🟣SMA-RSI: {}",&rsi_value);
                
            }
            
            if let Some(atr_value) = self.get_atr_normalized(price){
                println!("🔴ATR (NORMALIZED) : {}", atr_value);
                println!("🔴ATR (RAW) : {}", self.get_atr().unwrap());
            }

            if let Some(adx_value) = self.get_adx(){
                println!("🟡ADX : {}", adx_value);
            }

            if let Some(ema) = self.get_ema(){
                println!("🟠EMA: {}", ema);
            }

            if let Some(trend) = self.get_ema_cross_trend(){
                println!("DOUBLE EMA uptrend: {}", trend );
            }

            
        }




    
}


pub enum EngineCommand{

    UpdatePrice(PriceData),
    UpdateStrategy(Strategy),
    UpdateConfig(IndicatorsConfig),
    Reload(Vec<Price>),
}



    
