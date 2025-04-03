use hyperliquid_rust_sdk::{BaseUrl, InfoClient, Message, Subscription};
use tokio::sync::mpsc::UnboundedReceiver;
use std::time::{SystemTime, UNIX_EPOCH};
use kwant::indicators::{Price};

pub async fn subscribe_candles(
    session_time_secs: u64,
    coin: &str,
    tf: &str,
) -> (UnboundedReceiver<Message>, u32) {
    let mut info_client = InfoClient::new(None, Some(BaseUrl::Mainnet)).await.unwrap();
    
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();

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
    
    // Auto-unsubscribe
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(session_time_secs)).await;
        println!("Unsubscribing from candle data");
        let _ = info_client.unsubscribe(subscription_id).await;
    });

    (receiver, subscription_id)
}




fn get_time_now_and_candles_ago(candle_count: u64, tf: u64) -> (u64, u64) {
    let end = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    
    let interval = candle_count * tf * 60 * 1000; 
    let start = end - interval;

    (end, start)
}

pub async fn candles_snapshot(info_client: &InfoClient,coin: &str,time_frame: &str, start: u64, end: u64) -> Result<Vec<Price>, String>{
    
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

pub fn tf_to_minutes(tf: &str) -> Result<u64, String> {
    match tf {
        "1m" => Ok(1),
        "3m" => Ok(3),
        "5m" => Ok(5),
        "15m" => Ok(15),
        "30m" => Ok(30),
        "1h" => Ok(60),
        "2h" => Ok(120),
        "4h" => Ok(240),
        "8h" => Ok(480),
        "12h" => Ok(720),
        "1d" => Ok(1440),
        "3d" => Ok(4320),
        "w" => Ok(10080),
        "m" => Ok(43200),
        _ => Err(format!("Unsupported timeframe: {}", tf)),
    }
}


pub async fn load_candles(info_client: &InfoClient,coin: &str,tf: &str, candle_count: u64) -> Result<Vec<Price>, String> {

    let tf_u64 = tf_to_minutes(tf)?;

    let (end,start) = get_time_now_and_candles_ago(candle_count + 1, tf_u64);

    let price_data = candles_snapshot(info_client, coin, tf, start, end).await?;

    Ok(price_data)
}


