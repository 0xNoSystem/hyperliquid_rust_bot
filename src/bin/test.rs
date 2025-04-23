#![allow(unused_imports)]
#![allow(unused_mut)]
#![allow(unused_variables)]

use hyperliquid_rust_bot::strategy::{Strategy};
use hyperliquid_rust_bot::{BackTester, IndicatorsConfig, TradeParams};





#[tokio::main]
async fn main(){

    let params = TradeParams::default();
    let config = IndicatorsConfig::default();

    let mut bt = BackTester::new("SOL", params, Some(config), 1000.0);
    
    bt.run(3000).await;
  
    if let Some(value) = bt.signal_engine.get_rsi(){
         println!("RSI: {}", value);
    }


    
}
