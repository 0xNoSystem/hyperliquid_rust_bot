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
use hyperliquid_rust_bot::Market;
use hyperliquid_rust_bot::trade_setup::{TradeParams, Strategy, Risk};
use hyperliquid_rust_bot::helper::{subscribe_candles, load_candles};
use hyperliquid_rust_bot::{SignalEngine, IndicatorsConfig};

use flume::{bounded, TrySendError};


const COIN: &str = "FARTCOIN";

#[tokio::main]
async fn main(){
     dotenv().ok();
    env_logger::init();
   
    let wallet: LocalWallet = env::var("PRIVATE_KEY").expect("Error fetching PRIVATE_KEY")
        .parse()
        .unwrap();

    let pubkey: String = env::var("WALLET").expect("Error fetching WALLET address");
    
    let trade_params = TradeParams::default();
    let mut market = Market::new(wallet, pubkey, COIN.to_string(), trade_params, None).await.unwrap();

    match market.start().await{
        Ok(_) => println!("Market started"),
        Err(e) => error!("Error starting market: {}", e),
    };


}
