use hyperliquid_rust_sdk::InfoClient;
use kwant::indicators::{Rsi, Atr, Price, Indicator};
use crate::trade_setup::Strategy;

pub struct SignalEngine{
    info_client: InfoClient,
    rsi: Rsi,
    atr: Atr,
    config: Strategy,
    
}


pub struct IndicatorsConfig{
    pub rsi_length: usize,
    pub rsi_smoothing: Option<usize>,
    pub atr_length: usize,
}

impl SignalEngine{

    pub async fn new(config: IndicatorsConfig, strategy: Strategy) -> Self{
        
        SignalEngine{
            info_client: InfoClient::with_reconnect(None, Some(BaseUrl::Mainnet)).await.unwrap(),
            rsi: Rsi::new(config.rsi_length, config.rsi_smoothing),
            atr: Atr::new(config.atr_length),
            config: strategy,
        }
    }

}





impl Indicator for SignalEngine{

    fn update_after_close(&mut self, close: Price){

        self.rsi.update_after_close(close);
        self.atr.update_after_close(close);
    }

    fn update_before_close(&mut self, price: Price){

        self.rsi.update_before_close(price);
        self.atr.update_before_close(price);
    }

    fn is_ready(&self) -> bool{

       self.rsi.is_ready() && self.atr.is_ready()
    }

    fn load(&mut self, price_data: &Vec<Price>){

        self.rsi.load(price_data);
        self.atr.load(price_data);
    }

    fn get_last(&self) -> Option<f32>{

        None
    }
    
    fn reset(&mut self){
        self.rsi.reset();
        self.atr.reset();
    }
}
        
