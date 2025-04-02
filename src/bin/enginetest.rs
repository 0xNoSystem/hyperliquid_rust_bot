#![allow(unused_imports)]
#![allow(unused_mut)]
#![allow(unused_variables)]

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
use hyperliquid_rust_bot::signal::{SignalEngine, IndicatorsConfig};

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
    let mut info_client2 = InfoClient::new(None, Some(BaseUrl::Mainnet)).await.unwrap();
    
    let exchange_client = ExchangeClient::new(None, wallet.clone(), Some(BaseUrl::Mainnet), None, None)
        .await
        .unwrap();

    let indicators_config = IndicatorsConfig::default();

    let mut signal_engine = SignalEngine::new(
        indicators_config,
        Strategy::Neutral,
    ).await;

    signal_engine.load(&load_candles(&info_client, COIN, TF, 5000).await);

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
            
            if time != next_close{
                if candle_count == 0{
                    signal_engine.update(price, false);
                }
                candle_count += 1;
                signal_engine.update(price, true);
                time = next_close;
            }else{
                signal_engine.update(price, false);    
            }
            

            if let Some(stoch_rsi) = signal_engine.get_stoch_rsi(){
                println!("🔵STOCH-K: {}", stoch_rsi);
            }
            
            if let Some(stoch_rsi) = signal_engine.get_stoch_signal(){
                println!("🟠STOCH-D: {}", stoch_rsi);
            }

            if let Some(rsi_value) = signal_engine.get_rsi(){
                println!("🟣RSI: {}",&rsi_value);
                let _ = tx.try_send(MarketCommand::ExecuteTrade { size: SIZE, rsi: rsi_value});
                    
            };

            if let Some(sma_rsi) = signal_engine.get_sma_rsi(){
                println!("🟢SMA-RSI: {}", sma_rsi);
            }
            
            if let Some(atr_value) = signal_engine.get_atr(){
                println!("🔴ATR : {}", atr_value);
            }

            if let Some(adx_value) = signal_engine.get_adx(){
                println!("🟡ADX : {}", adx_value);
            }

            if let Some(ema) = signal_engine.get_ema(){
                println!("🟠EMA: {}", ema);
            }

            if let Some(trend) = signal_engine.get_ema_cross_trend(){
                println!("EMA CROSS UPTREND: {}", trend );
            }

            if let Some(ema_slope) = signal_engine.get_ema_slope(){
                println!("EMA SLOPE: {}", ema_slope);
            }


        }

    }

}