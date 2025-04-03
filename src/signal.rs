use hyperliquid_rust_sdk::{InfoClient, BaseUrl};
use kwant::indicators::{Rsi, Atr, Price, Indicator, Ema, EmaCross, Sma, Adx};
use crate::trade_setup::Strategy;
use crate::{MAX_HISTORY};

#[derive(Debug)]
pub struct SignalEngine{
    info_client: InfoClient,
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

    pub async fn new(config: IndicatorsConfig, strategy: Strategy) -> Self{
        
        let ema_cross = config.ema_cross_short_long_lenghts
        .map(|(short, long)| EmaCross::new(short, long));

        SignalEngine{
            info_client: InfoClient::with_reconnect(None, Some(BaseUrl::Mainnet)).await.unwrap(),
            indicators_config: config.clone(),
            rsi: Rsi::new(config.rsi_length, config.stoch_rsi_length ,config.rsi_smoothing),
            atr: Atr::new(config.atr_length),
            ema: Ema::new(config.ema_length),
            ema_cross: ema_cross,
            adx: Adx::new(config.adx_length, config.adx_length),
            sma: Sma::new(config.sma_length),
            strategy,
            price_data: Vec::with_capacity(MAX_HISTORY),
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
    
    pub fn get_last_close(&self) -> Option<Price>{
        self.price_data.last().cloned()
    }

    
}


impl SignalEngine{
    //
}