#![allow(unused_imports)]
#[allow(unused_mut)]

use ethers::signers::LocalWallet;
use ethers::types::H160;

use log::info;
use std::env;
use dotenv::dotenv;

use hyperliquid_rust_sdk::{
    ExchangeClient, ExchangeDataStatus, ExchangeResponseStatus,
    MarketOrderParams,
};
use hyperliquid_rust_sdk::{BaseUrl, InfoClient, Message, Subscription};
use tokio::{
    spawn,
    sync::mpsc::{unbounded_channel},
    time::{sleep, Duration},
};

use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

use indicators::rsi2::Rsi;
use hyperliquid_rust_bot::bot::Bot;
use hyperliquid_rust_bot::trade_setup::{TradeParams, Strategy, Risk};


const SIZE: f32 = 50.0;
const COIN: &str = "SOL";

#[tokio::main]
async fn main(){
    dotenv().ok();
    env_logger::init();
    
    let wallet: LocalWallet = env::var("PRIVATE_KEY").expect("Error fetching PRIVATE_KEY")
        .parse()
        .unwrap();

    let pubkey: String = env::var("WALLET").expect("Error fetching WALLET address");
    let mut info_client = InfoClient::new(None, Some(BaseUrl::Testnet)).await.unwrap();
    let exchange_client = ExchangeClient::new(None, wallet.clone(), Some(BaseUrl::Testnet), None, None)
        .await
        .unwrap();

    let trade_params = TradeParams {
        strategy: Strategy::Neutral,
        risk: Risk::Medium,
        lev: 13,
        trade_time: 200,
        asset: COIN.into(),        
        time_frame: "1m".into(),    
    };

    let mut bot = Bot::new(
        wallet.clone(),
        pubkey,
        info_client,
        exchange_client,
        trade_params,
    );
    bot.init().await;


        if bot.is_active() == false{
            tokio::spawn(async move {bot.trade_exec(SIZE, false).await;});
        }
    
        for i in 0..50{
            print!("{}",i);
        }
}


