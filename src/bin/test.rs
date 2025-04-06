#![allow(unused_imports)]
#![allow(unused_mut)]
#![allow(unused_variables)]

use hyperliquid_rust_bot::Executor;
use hyperliquid_rust_bot::trade_setup::{TradeFillInfo, TradeInfo, TradeCommand};

use ethers::signers::LocalWallet;
use ethers::types::H160;
use log::info;
use std::{thread,env};
use dotenv::dotenv;

use hyperliquid_rust_sdk::{
    ExchangeClient, ExchangeDataStatus, ExchangeResponseStatus,
    MarketOrderParams, UserFillsResponse,
};
use hyperliquid_rust_sdk::{BaseUrl, InfoClient, Message, Subscription};
use tokio::{
    sync::mpsc::{unbounded_channel,UnboundedReceiver},
    time::{sleep, Duration},
};

use hyperliquid_rust_bot::helper::user_fills;


#[tokio::main]
async fn main(){
     dotenv().ok();
    env_logger::init();
  
    //let pubkey: String = env::var("WALLET").expect("Error fetching WALLET address");
    
    let wallet: LocalWallet = env::var("PRIVATE_KEY").expect("Error fetching PRIVATE_KEY")
    .parse()
    .unwrap();
    
    let pubkey: String = env::var("WALLET").expect("Error fetching WALLET address");
    let exchange_client = ExchangeClient::new(None, wallet.clone(), Some(BaseUrl::Mainnet), None, None).await.unwrap();
    
    
    let mut info_client = InfoClient::new(None, Some(BaseUrl::Mainnet)).await.unwrap();

    let mut exec = Executor::new("SOL".to_string(), exchange_client);
    

    let trade = TradeCommand{
        size: 1.0,
        is_long: false,
        duration: 30,
         
    };
     
    let status =  exec.open_order(trade).await;
   // println!("STATUS OPEN: {}", status.unwrap().oid);
    let fill = user_fills(&info_client,pubkey.clone()).await;
    let open_fee = fill[0].fee.parse::<f32>().unwrap(); 


    let _ = sleep(Duration::from_secs(trade.duration)).await; 

    let status_close =  exec.close_order(trade).await;
    println!("STATUS CLOSE: {}", status_close.unwrap().oid);

    let fill2 = user_fills(&info_client,pubkey).await;
    let close_fee = fill2[0].fee.parse::<f32>().unwrap(); 
    let close_pnl = fill2[0].closed_pnl.parse::<f32>().unwrap();

    let pnl = close_pnl - open_fee - close_fee;
    println!("PNL: {}", pnl);
     


}   
