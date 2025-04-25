use hyperliquid_rust_sdk::{AssetMeta, BaseUrl,ExchangeClient, InfoClient, Message, Subscription, UserFillsResponse};
use tokio::sync::mpsc::{UnboundedReceiver};
use tokio::sync::watch::{Sender, Receiver};
use std::time::{SystemTime, UNIX_EPOCH};
use kwant::indicators::{Price};
use ethers::types::H160;
use serde::Deserialize;
use crate::TimeFrame;
use log::warn;

pub async fn subscribe_candles(
    url: BaseUrl,
    coin: &str,
    tf: &str,
) -> (Sender<bool>,UnboundedReceiver<Message>) {
    let mut info_client = InfoClient::with_reconnect(None, Some(url)).await.unwrap();
    
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);



    let subscription_id = info_client
        .subscribe(
            Subscription::Candle {
                coin: coin.to_string(),
                interval: tf.to_string(),
            },
            sender,
        )
        .await
        .unwrap();
    println!("Subscribed to candle data: {:?}", subscription_id);
    
    tokio::spawn(async move {
        while shutdown_rx.changed().await.is_ok() {
            if *shutdown_rx.borrow() {
                println!("Shutdown received, unsubscribing...");
                let _ = info_client.unsubscribe(subscription_id).await;
                break;
            }
        }
    }); 

    (shutdown_tx, receiver)
}




fn get_time_now_and_candles_ago(candle_count: u64, tf: TimeFrame) -> (u64, u64) {
    let end = get_time_now();

    let interval = candle_count
    .checked_mul(tf.to_secs())
    .and_then(|s| s.checked_mul(1_000))
    .expect("interval overflowed");

    let start = end.saturating_sub(interval);

    (start, end)
}



async fn candles_snapshot(info_client: &InfoClient,coin: &str,time_frame: TimeFrame, start: u64, end: u64) -> Result<Vec<Price>, String>{
 
    let vec = match info_client
    .candles_snapshot(coin.to_string(), time_frame.to_string(), start, end)
    .await
    {
        Ok(data) => data,
        Err(e) => {
            eprintln!("Failed to fetch candles: {}", e);
            return Err("Candles Snapshot Failed".to_string());
        }
    };
    let mut res = Vec::new();
    for candle in vec {
        let h = candle.high.parse::<f32>().map_err(|e| e.to_string())?;
        let l = candle.low.parse::<f32>().map_err(|e| e.to_string())?;
        let o = candle.open.parse::<f32>().map_err(|e| e.to_string())?;
        let c = candle.close.parse::<f32>().map_err(|e| e.to_string())?;

        res.push(Price {
            high: h,
            low: l,
            open: o,
            close: c,
    });
    }
    Ok(res)
}


pub async fn load_candles(info_client: &InfoClient,coin: &str,tf: TimeFrame, candle_count: u64) -> Result<Vec<Price>, String> {


    let (start, end) = get_time_now_and_candles_ago(candle_count + 1, tf);

    let price_data = candles_snapshot(info_client, coin, tf, start, end).await?;

    Ok(price_data)
}




pub fn address(address: &String) -> H160 {
    address.parse().unwrap()
}



pub async fn get_max_lev(info_client: &InfoClient, token: &str) -> u32{
    let assets = info_client.meta().await.unwrap().universe;

    if let Some(asset) = assets.iter().find(|a| a.name == token) {
        asset.max_leverage
    }else{
        warn!("ERROR: Failed to retrieve max_leverage for {}", token);
        1
    }
}


pub async fn get_asset(info_client: &InfoClient, token: &str) -> Option<AssetMeta>{
    let assets = info_client.meta().await.unwrap().universe;

    if let Some(asset) = assets.into_iter().find(|a| a.name == token) {
        Some(asset)
    }else{
        None
    }
}


pub fn get_time_now() -> u64{
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
}
    

