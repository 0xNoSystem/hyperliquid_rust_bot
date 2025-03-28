#![allow(unused_imports)]
#![allow(unused_mut)]

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
    sync::mpsc::{unbounded_channel},
    time::{sleep, Duration},
};

use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

use indicators::rsi2::Rsi;
use hyperliquid_rust_bot::bot::{Bot, BotCommand};
use hyperliquid_rust_bot::trade_setup::{TradeParams, Strategy, Risk};


const SIZE: f32 = 50.0;
const COIN: &str = "SOL";
const TF: &str = "5m";

#[tokio::main]
async fn main(){
    dotenv().ok();
    env_logger::init();
    
    let wallet: LocalWallet = env::var("PRIVATE_KEY").expect("Error fetching PRIVATE_KEY")
        .parse()
        .unwrap();

    let pubkey: String = env::var("WALLET").expect("Error fetching WALLET address");
    let mut info_client = InfoClient::new(None, Some(BaseUrl::Testnet)).await.unwrap();
    let  mut info_client2 = InfoClient::new(None, Some(BaseUrl::Testnet)).await.unwrap();
    let exchange_client = ExchangeClient::new(None, wallet.clone(), Some(BaseUrl::Testnet), None, None)
        .await
        .unwrap();

    let trade_params = TradeParams {
        strategy: Strategy::Neutral,
        risk: Risk::Medium,
        lev: 20,
        trade_time: 600,
        asset: COIN.to_string(),        
        time_frame: TF.to_string(),    
    };

    let mut bot = Bot::new(
        wallet.clone(),
        pubkey,
        info_client,
        exchange_client,
        trade_params,
    );
    bot.init().await;
    
    let (tx, mut rx) = tokio::sync::mpsc::channel(32);
        tokio::spawn(async move{
            while let Some(cmd) = rx.recv().await {
                println!("HEY");
            match cmd {
                BotCommand::ExecuteTrade { size, rsi } => {
                    
                    let signal = bot.get_signal(rsi).await;
                    bot.trade_exec(size, signal).await;
                }
            }
        }
        });
    
    let mut rsi = Rsi::new(12, 10);
    
    
    let (sender, mut receiver) = unbounded_channel();

    let subscription_id = info_client2
        .subscribe(
            Subscription::Candle{
                coin: COIN.to_string(),
                interval: TF.to_string(),
            },
            sender,
        )
        .await
        .unwrap();

    tokio::spawn(async move {
        sleep(Duration::from_secs(30000)).await;
        info!("Unsubscribing from candle data");
        info_client2.unsubscribe(subscription_id).await.unwrap();
        
    });

    let mut time = 0;
    let mut candle_count = 0;
    while let Some(Message::Candle(candle)) = receiver.recv().await {
        
        let price = candle.data.close.parse::<f32>().ok();
        let next_close =  candle.data.time_close;
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

            if let Some(rsi_value) = rsi.get_last(){
                println!("Price: {}\nRSI: {}", &close, &rsi_value);
                let _ = tx.send(BotCommand::ExecuteTrade { size: SIZE, rsi: rsi_value}).await;
            }

        }

        println!("\nCandle => {}", candle_count);
        println!("{:?}", price);

    }


}


