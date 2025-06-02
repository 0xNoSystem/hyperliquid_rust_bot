#![allow(unused_imports)]
#![allow(unused_mut)]
#![allow(unused_variables)]
#![allow(dead_code)]

use std::{
    env, fs,
    str::FromStr,
    sync::{Arc, atomic::{AtomicBool, Ordering}},
    thread,
};

use dotenv::dotenv;
use ethers::signers::LocalWallet;
use flume::{bounded, TrySendError};
use hyperliquid_rust_sdk::{
    BaseUrl, ExchangeClient, ExchangeDataStatus, ExchangeResponseStatus,
    InfoClient, MarketOrderParams, Message, Subscription, Error
};
use kwant::indicators::{Price, Rsi, Indicator};
use log::{error, info};
use tokio::{
    sync::mpsc::{unbounded_channel, UnboundedReceiver},
    time::{sleep, Duration},
};

use hyperliquid_rust_bot::{
    Wallet, Market,MarketCommand, Executor, SignalEngine, IndexId, IndicatorKind, EditType, Entry
};
use hyperliquid_rust_bot::trade_setup::{TimeFrame, TradeInfo, TradeParams};
use hyperliquid_rust_bot::strategy::{Strategy, CustomStrategy, Style, Stance, Risk};
use hyperliquid_rust_bot::helper::{subscribe_candles, load_candles};



const COIN: &str = "LAUNCHCOIN";
const URL: BaseUrl = BaseUrl::Mainnet;

#[tokio::main]
async fn main() -> Result<(), Error>{
    use IndicatorKind::*;
    env_logger::init();
    match URL{
        BaseUrl::Mainnet => dotenv().ok(),
        BaseUrl::Testnet => dotenv::from_filename("testnet").ok(),
        BaseUrl::Localhost => dotenv::from_filename(".env.test").ok(),
        };
        
    /// 
    let wallet: LocalWallet = env::var("PRIVATE_KEY").expect("Error fetching PRIVATE_KEY")
        .parse()
        .unwrap();
    let pubkey: String = env::var("WALLET").expect("Error fetching WALLET address");

    let wallet = Wallet::new(URL, wallet).await?; 
   
    let strat = Strategy::Custom(CustomStrategy::default());
   
    let trade_params = TradeParams{
        strategy: strat,
        lev: 20,
        trade_time: 300,
        time_frame: TimeFrame::from_str("1m").unwrap_or(TimeFrame::Min1),
    
    };

    let config = Vec::from([
    (
        IndicatorKind::Rsi(12),
        TimeFrame::Min1,
    ),
        (
        IndicatorKind::SmaOnRsi{periods: 14, smoothing_length: 9},
        TimeFrame::Hour1,
    ),
    (
        IndicatorKind::StochRsi{periods: 16,k_smoothing: Some(4), d_smoothing: Some(4)},
        TimeFrame::Hour4,
    ),
      
    (
        IndicatorKind::EmaCross{short: 21, long: 200},
        TimeFrame::Day1,
    ),  
    (
        IndicatorKind::Adx {
            periods: 14,
            di_length: 14,
        },
        TimeFrame::Min5,
    ),
    (
        IndicatorKind::Atr(14),
        TimeFrame::Min15,
    ),
    (
        IndicatorKind::Sma(50),
        TimeFrame::Hour1,
    ),
]);

    let (mut market, sender) = Market::new(wallet,COIN.trim().to_string(), trade_params, Some(config)).await.unwrap();

   tokio::spawn(async move{
        
        /*let _ = sleep(Duration::from_secs(10)).await;
        sender.send(MarketCommand::UpdateLeverage(50)).await;
        let _ = sleep(Duration::from_secs(10)).await;
        sender.send(MarketCommand::UpdateLeverage(40)).await;*/

        //let _ = sleep(Duration::from_secs(120)).await;
        //sender.send(MarketCommand::Pause).await;
        let _ = sleep(Duration::from_secs(20)).await;
        //sender.send(MarketCommand::UpdateTimeFrame(TimeFrame::from_str("4h").unwrap())).await;
        let _ = sender.send(MarketCommand::EditIndicators(Vec::from([Entry{id: (Ema(33), TimeFrame::Hour1),edit: EditType::Add}, 
                                                            Entry{id: (SmaOnRsi{periods: 12, smoothing_length: 9}, TimeFrame::Min1),edit: EditType::Add}
        ]))).await;

        let _ = sleep(Duration::from_secs(20)).await;
        let _ = sender.send(MarketCommand::EditIndicators(Vec::from([Entry{id: (Ema(33), TimeFrame::Hour4),edit: EditType::Add}]))).await;
        let _ = sleep(Duration::from_secs(10)).await;
        let _ = sender.send(MarketCommand::EditIndicators(Vec::from([Entry{id: (Ema(33), TimeFrame::Hour1),edit: EditType::Toggle}]))).await;

        let _ = sleep(Duration::from_secs(20)).await;
        let _ = sender.send(MarketCommand::EditIndicators(Vec::from([Entry{id: (Sma(10), TimeFrame::Min5),edit: EditType::Add}]))).await;
        let _ = sleep(Duration::from_secs(20)).await;
        sender.send(MarketCommand::EditIndicators(Vec::from([Entry{id: (Ema(10), TimeFrame::Hour4),edit: EditType::Remove}, 
                                                            Entry{id: (Sma(10), TimeFrame::Min5),edit: EditType::Remove},
                                                            Entry{id: (Atr(14), TimeFrame::Min15),edit: EditType::Remove},
                                                            Entry{id: (Rsi(12), TimeFrame::Min1),edit: EditType::Toggle}
        ]))).await;
        let _ = sleep(Duration::from_secs(30)).await;
        let _ = sender.send(MarketCommand::EditIndicators(Vec::from([]))).await;
        let _ =sender.send(MarketCommand::EditIndicators(Vec::from([Entry{id: (Rsi(12), TimeFrame::Min1),edit: EditType::Toggle}]))).await;
        let _ = sleep(Duration::from_secs(100000)).await;
        sender.send(MarketCommand::Close).await;
        //let _ = sleep(Duration::from_secs(30)).await;
        //let _ = sender.send(MarketCommand::Close).await; 
});

    
    market.start().await?;
    Ok(())
}




fn load_strategy(path: &str) -> Strategy {
    let content = fs::read_to_string(path).expect("failed to read file");
    toml::from_str(&content).expect("failed to parse toml")
}










