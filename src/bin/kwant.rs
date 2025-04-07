#![allow(unused_imports)]
#![allow(unused_mut)]
#![allow(unused_variables)]

use hyperliquid_rust_bot::helper::load_candles;
use kwant::indicators::{Rsi, Indicator, Price, Ema, Adx, Atr, EmaCross};
use hyperliquid_rust_sdk::{InfoClient, BaseUrl};
use tokio::time::{Duration, sleep};

#[tokio::main]
async fn main(){
    let mut info_client = InfoClient::new(None, Some(BaseUrl::Mainnet)).await.unwrap();
}
