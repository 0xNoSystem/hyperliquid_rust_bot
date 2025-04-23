#![allow(unused_imports)]
#![allow(unused_mut)]
#![allow(unused_variables)]

use ethers::signers::LocalWallet;
use ethers::types::H160;
use log::info;
use log::error;
use std::{thread,env, fs};
use toml;
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
use std::str::FromStr;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use kwant::indicators::{Rsi,Price, Indicator};
use hyperliquid_rust_bot::{Wallet, Market, MarketCommand};
use hyperliquid_rust_bot::trade_setup::{TimeFrame,TradeInfo, TradeParams};
use hyperliquid_rust_bot::strategy::{Strategy, CustomStrategy, Style, Stance, Risk};
use hyperliquid_rust_bot::helper::{subscribe_candles, load_candles};
use hyperliquid_rust_bot::{SignalEngine, IndicatorsConfig};

use flume::{bounded, TrySendError};


const COIN: &str = "BTC ";
const URL: BaseUrl = BaseUrl::Testnet;

#[tokio::main]
async fn main(){
    env_logger::init();
    match URL{
        BaseUrl::Mainnet => dotenv().ok(),
        BaseUrl::Testnet => dotenv::from_filename(".env.test").ok(),
        BaseUrl::Localhost => dotenv::from_filename(".env.test").ok(),
        };
        
    /// 
    let wallet: LocalWallet = env::var("PRIVATE_KEY").expect("Error fetching PRIVATE_KEY")
        .parse()
        .unwrap();
    let pubkey: String = env::var("WALLET").expect("Error fetching WALLET address");

    let wallet = Wallet::new(URL, pubkey, wallet).await; 
   

    let strat = Strategy::Custom(CustomStrategy::default());
   
    let trade_params = TradeParams{
        strategy: strat,
        lev: 20,
        trade_time: 300,
        time_frame: TimeFrame::from_str("1m").unwrap_or(TimeFrame::Min1),
    
    };

    let (mut market, sender) = Market::new(wallet,COIN.trim().to_string(), trade_params, None).await.unwrap();


    let config = IndicatorsConfig {
    rsi_length: 9,
    rsi_smoothing: Some(8),
    stoch_rsi_length: 10,
    atr_length: 30,
    ema_length: 18,
    ema_cross_short_long_lenghts: Some((8, 21)),
    adx_length: 5,
    sma_length: 100,
};

   tokio::spawn(async move{
        
        let _ = sleep(Duration::from_secs(10)).await;
        sender.send(MarketCommand::UpdateLeverage(50)).await;
        let _ = sleep(Duration::from_secs(10)).await;
        sender.send(MarketCommand::UpdateLeverage(40)).await;
        let _ = sleep(Duration::from_secs(10)).await;
        sender.send(MarketCommand::UpdateIndicatorsConfig(config)).await;

        let _ = sleep(Duration::from_secs(120)).await;
        sender.send(MarketCommand::Pause).await;
        let _ = sleep(Duration::from_secs(5)).await;
        sender.send(MarketCommand::UpdateTimeFrame(TimeFrame::from_str("4h").unwrap())).await;
        sender.send(MarketCommand::UpdateIndicatorsConfig(IndicatorsConfig::default())).await;

        let _ = sleep(Duration::from_secs(10)).await;
        sender.send(MarketCommand::Pause).await;

        let _ = sleep(Duration::from_secs(3000000)).await;
        sender.send(MarketCommand::Close).await;
        //let _ = sleep(Duration::from_secs(30)).await;
        //let _ = sender.send(MarketCommand::Close).await; 
});



    match market.start().await{
        Ok(_) => println!("Market closed successfully"),
        Err(e) => error!("Error starting market: {}", e),
    };
    
    

}




fn load_strategy(path: &str) -> Strategy {
    let content = fs::read_to_string(path).expect("failed to read file");
    toml::from_str(&content).expect("failed to parse toml")
}










