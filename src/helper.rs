use hyperliquid_rust_sdk::{BaseUrl,ExchangeClient, InfoClient, Message, Subscription, UserFillsResponse};
use tokio::sync::mpsc::{UnboundedReceiver};
use tokio::sync::watch::{Sender, Receiver};
use std::time::{SystemTime, UNIX_EPOCH};
use kwant::indicators::{Price};
use ethers::types::H160;
use serde::Deserialize;
use crate::TimeFrame;

pub async fn subscribe_candles(
    coin: &str,
    tf: &str,
) -> (Sender<bool>,UnboundedReceiver<Message>) {
    let mut info_client = InfoClient::with_reconnect(None, Some(BaseUrl::Mainnet)).await.unwrap();
    
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

async fn get_user_margin(info_client: &InfoClient, user: String) -> Result<f32, String> {
        let user = address(user);

        let info = info_client.user_state(user)
        .await
        .map_err(|e| format!("Error fetching user balance, {}",e))?;

        let res =  info.cross_margin_summary.account_value
        .parse::<f32>()
        .map_err(|e| format!("FATAL: failed to parse account balance to f32, {}",e))?;
        Ok(res) 
}




fn get_time_now_and_candles_ago(candle_count: u64, tf: TimeFrame) -> (u64, u64) {
    let end = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    
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




pub async fn user_fills(info_client: &InfoClient, user: String) -> Vec<UserFillsResponse>{

    let user = address(user);

    return info_client.user_fills(user).await.unwrap();
    
}

pub fn address(address: String) -> H160 {
    address.parse().unwrap()
}





pub async fn get_user_fees(info_client: &InfoClient, user: String) -> (f32, f32) {
    let user = address(user);
    let user_fees = info_client.user_fees(user).await.unwrap();
    let add_fee: f32 = user_fees.user_add_rate.parse().unwrap();
    let cross_fee: f32 = user_fees.user_cross_rate.parse().unwrap();
    
    (add_fee, cross_fee)
}










