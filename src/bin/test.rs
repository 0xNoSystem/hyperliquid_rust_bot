#![allow(unused_imports)]
#![allow(unused_mut)]

use ethers::signers::LocalWallet;
use ethers::types::H160;
use log::info;
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

use indicators::rsi2::Rsi;
use hyperliquid_rust_bot::bot::{Bot, BotCommand};
use hyperliquid_rust_bot::trade_setup::{TradeParams, Strategy, Risk};
use hyperliquid_rust_bot::helper::subscribe_candles;

use flume::{bounded, TrySendError};

const SIZE: f32 = 1.0;
const COIN: &str = "SOL";
const TF: &str = "1m";

#[tokio::main]
async fn main(){
    dotenv().ok();
    env_logger::init();
    
    let wallet: LocalWallet = env::var("PRIVATE_KEY").expect("Error fetching PRIVATE_KEY")
        .parse()
        .unwrap();

    let pubkey: String = env::var("WALLET").expect("Error fetching WALLET address");
    let mut info_client = InfoClient::new(None, Some(BaseUrl::Mainnet)).await.unwrap();
    
    let exchange_client = ExchangeClient::new(None, wallet.clone(), Some(BaseUrl::Mainnet), None, None)
        .await
        .unwrap();

    let trade_params = TradeParams {
        strategy: Strategy::Neutral,
        risk: Risk::Low,
        lev: 20,
        trade_time: 480,
        asset: COIN.to_string(),        
        time_frame: TF.to_string(),    
    };

    let mut bot = Bot::new(
        wallet,
        pubkey,
        info_client,
        exchange_client,
        trade_params,
    );
    bot.init().await;
 
    let (tx, rx) = bounded::<BotCommand>(0);
   
    tokio::spawn(async move {
    while let Ok(cmd) = rx.recv_async().await {
        match cmd {
            BotCommand::ExecuteTrade { size, rsi } => {
                let signal = bot.get_signal(rsi).await;
                bot.trade_exec(size, signal).await;
            }
        }
    }
    });
        
    let mut rsi = Rsi::new(12, 10);
    let (mut receiver, _subscription_id) = subscribe_candles(30000,COIN, TF).await;

    let mut time = 0;
    let mut candle_count = 0;
    while let Some(Message::Candle(candle)) = receiver.recv().await {
        
        let price = candle.data.close.parse::<f32>().ok();
        let next_close =  candle.data.time_close;
        println!("\nCandle => {}", candle_count);
        println!("Price: {}$", price.unwrap());
        if let Some(close) = price{

            if time != next_close {
                candle_count += 1;
                rsi.update_after_close(close);
                time = next_close;
            }else{
                if rsi.is_ready(){
                    rsi.update_before_close(close);
                }
            }
            
            if let Some(sma_on_rsi) = rsi.get_sma_rsi(){
                println!("SMA_ON_RSI: {}", sma_on_rsi);
            }
            
            if let Some(rsi_value) = rsi.get_last(){
                println!("RSI: {}",&rsi_value);
                let _ = tx.try_send(BotCommand::ExecuteTrade { size: SIZE, rsi: rsi_value});
                    
            };

        }

    }
}
