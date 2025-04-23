use crate::{SignalEngine, IndicatorsConfig, MARKETS };  
use crate::helper::{load_candles};
use kwant::indicators::{Price};
use crate::trade_setup::{TradeParams, TimeFrame};
use crate::strategy::{Strategy, CustomStrategy, Style, Stance};
use tokio::time::{sleep, Duration};
use hyperliquid_rust_sdk::{InfoClient, BaseUrl};

#[derive(Debug)]

pub struct BackTester{
    pub asset: String,
    pub signal_engine: SignalEngine,
    pub params: TradeParams,
    pub candle_data: Vec<Price>,
}




impl BackTester{

    pub fn new(asset: &str,params: TradeParams, config: Option<IndicatorsConfig>, margin: f32) -> Self{
        if !MARKETS.contains(&asset){
            panic!("ASSET ISN'T TRADABLE, MARKET CAN'T BE INITILIAZED");
        }



        BackTester{
            asset: asset.to_string(),
            signal_engine: SignalEngine::new_backtest(params.clone(), config, margin),
            params,
            candle_data: Vec::new(),
        }
    }


    async fn load(&mut self, candle_count: u64) -> Result<(), String>{

        let mut info_client = InfoClient::new(None, Some(BaseUrl::Mainnet)).await.unwrap();
        let candle_data = load_candles(&info_client,
                                    self.asset.as_str(),
                                    self.params.time_frame,
                                    candle_count).await?;

        
        self.candle_data = candle_data;
        Ok(())
    }
    


    pub fn change_strategy(&mut self, strategy: Strategy){
    
        self.params.strategy = strategy;
        self.signal_engine.change_strategy(strategy);

    }


    pub async fn change_time_frame(&mut self, tf: TimeFrame) -> Result<(), String>{
    
        self.params.time_frame = tf;
        self.signal_engine.reset();
        
        self.load(self.candle_data.len() as u64).await?;
        Ok(())
    }


    pub fn change_indicators_config(&mut self, new_config: IndicatorsConfig){
        
        self.signal_engine.change_indicators_config(new_config);
    }



    pub async fn run(&mut self,candle_count: u64) -> bool{
        self.load(candle_count).await;
    
        let mut tick = 0;
        
        for price in &self.candle_data{
            println!("\nPrice: {}", price.close); 
            self.signal_engine.update_after_close(*price);

            if let Some(value) = self.signal_engine.get_rsi(){
                
                self.signal_engine.display_indicators(price.close); 
                let _ = sleep(Duration::from_millis(10)).await;
            }        

    }
        false
        
        
    }


}




#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn run(){

        let params = TradeParams::default();
        let mut bt = BackTester::new("SOL", params, None );


        let res = bt.run(3000, 1000).await;
        assert!(res);
       
        if let Some(value) = bt.signal_engine.get_rsi(){
            println!("RSI: {}", value);
        }else{
            assert!(false);
        }

    }

}













