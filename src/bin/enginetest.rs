#![allow(unused_imports)]
#![allow(unused_mut)]
#![allow(unused_variables)]

use ethers::signers::LocalWallet;
use ethers::types::H160;
use log::info;
use log::error;
use std::{thread,env};
use dotenv::dotenv;

use hyperliquid_rust_sdk::{
    ExchangeClient, ExchangeDataStatus, ExchangeResponseStatus,
    MarketOrderParams,
};
use hyperliquid_rust_sdk::{BaseUrl, InfoClient, Message, Subscription};
use tokio::{
    sync::mpsc::{unbounded_channel,UnboundedReceiver},
    time::{sleep, Duration},
};

use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use kwant::indicators::{Rsi,Price, Indicator};
use hyperliquid_rust_bot::{Market, MarketCommand};
use hyperliquid_rust_bot::trade_setup::{TradeParams, Strategy, Risk};
use hyperliquid_rust_bot::helper::{subscribe_candles, load_candles};
use hyperliquid_rust_bot::{SignalEngine, IndicatorsConfig};

use flume::{bounded, TrySendError};


const COIN: &str = "SOL";

#[tokio::main]
async fn main(){
     dotenv().ok();
    env_logger::init();
   
    let wallet: LocalWallet = env::var("PRIVATE_KEY").expect("Error fetching PRIVATE_KEY")
        .parse()
        .unwrap();

    let pubkey: String = env::var("WALLET").expect("Error fetching WALLET address");
    
    let trade_params = TradeParams::default();
    let (mut market, sender) = Market::new(wallet, pubkey, COIN.to_string(), trade_params, None).await.unwrap();


    let config = IndicatorsConfig {
    rsi_length: 14,
    rsi_smoothing: Some(5),
    stoch_rsi_length: 14,
    atr_length: 21,
    ema_length: 20,
    ema_cross_short_long_lenghts: Some((8, 21)),
    adx_length: 14,
    sma_length: 50,
};

   tokio::spawn(async move{
        /*
        let _ = sleep(Duration::from_secs(20)).await;
        sender.send(MarketCommand::UpdateLeverage(30)).await;
        let _ = sleep(Duration::from_secs(10)).await;
        sender.send(MarketCommand::UpdateIndicatorsConfig(config)).await;
        let _ = sleep(Duration::from_secs(10)).await;
        sender.send(MarketCommand::Close).await; */
        let _ = sleep(Duration::from_secs(10)).await;
        sender.send(MarketCommand::UpdateTimeFrame("15m".to_string())).await;
});



    match market.start().await{
        Ok(_) => println!("Market started"),
        Err(e) => error!("Error starting market: {}", e),
    };
    
    

}
