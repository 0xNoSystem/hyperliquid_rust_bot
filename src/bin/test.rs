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

use kwant::indicators::{Rsi,Price, Indicator};
use hyperliquid_rust_bot::market::{Market, MarketCommand};
use hyperliquid_rust_bot::trade_setup::{TradeParams, Strategy, Risk};
use hyperliquid_rust_bot::helper::{subscribe_candles, load_candles};

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
    let mut info_client = InfoClient::with_reconnect(None, Some(BaseUrl::Mainnet)).await.unwrap();
    
    let exchange_client = ExchangeClient::new(None, wallet.clone(), Some(BaseUrl::Mainnet), None, None)
        .await
        .unwrap();

    let mut rsi = Rsi::new(14, 14, None);
    rsi.load(&load_candles(&info_client, COIN, TF, rsi.period() as u64 *3).await);

    let trade_params = TradeParams {
        strategy: Strategy::Neutral,
        risk: Risk::Low,
        lev: 20,
        trade_time: 480,
        asset: COIN.to_string(),        
        time_frame: TF.to_string(),    
    };

    let mut market = Market::new(
        wallet,
        pubkey,
        info_client,
        exchange_client,
        trade_params,
    );
    market.init().await;
 
    let (tx, rx) = bounded::<MarketCommand>(0);
   
    tokio::spawn(async move {
    while let Ok(cmd) = rx.recv_async().await {
        match cmd {
            MarketCommand::ExecuteTrade { size, rsi } => {
                let signal = market.get_signal(rsi).await;
                market.market_trade_exec(size, signal).await;
            }
        }
    }
    });
        
    

    let (mut receiver, _subscription_id) = subscribe_candles(30000,COIN, TF).await;

    let mut time = 0;
    let mut candle_count = 0;
    while let Some(Message::Candle(candle)) = receiver.recv().await {
        
        let close = candle.data.close.parse::<f32>().ok().unwrap();
        let high = candle.data.high.parse::<f32>().ok().unwrap();
        let low = candle.data.low.parse::<f32>().ok().unwrap();
        let open = candle.data.open.parse::<f32>().ok().unwrap();

        let price = Price {open, high, low, close};

        let next_close =  candle.data.time_close;
        println!("\nCandle => {}", candle_count);
        println!("Price: {}$", close);
        {

            if time != next_close {
                candle_count += 1;
                rsi.update_after_close(price);
                time = next_close;
            }else{
                if rsi.is_ready(){
                    rsi.update_before_close(price);
                }
            }
            
            if let Some(stoch_rsi) = rsi.get_stoch_rsi(){
                println!("STOCH-K: {}", stoch_rsi);
            }
            
            if let Some(stoch_rsi) = rsi.get_stoch_signal(){
                println!("STOCH-D: {}", stoch_rsi);
            }
            if let Some(rsi_value) = rsi.get_last(){
                println!("RSI: {}",&rsi_value);
                let _ = tx.try_send(MarketCommand::ExecuteTrade { size: SIZE, rsi: rsi_value});
                    
            };

        }

    }
}
