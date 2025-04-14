#![allow(unused_imports)]
#![allow(unused_mut)]
#![allow(unused_variables)]

use hyperliquid_rust_bot::{BackTester, IndicatorsConfig, TradeParams, Strategy};





#[tokio::main]
async fn main(){

    let params = TradeParams::default();

    let mut bt = BackTester::new("SOL", params, None);
    
    bt.run(3000, 1000).await;
  
    if let Some(value) = bt.signal_engine.get_rsi(){
         println!("RSI: {}", value);
    }


    
}
