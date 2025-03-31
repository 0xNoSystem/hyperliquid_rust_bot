use hyperliquid_rust_sdk::{BaseUrl, InfoClient, Message, Subscription};
use tokio::sync::mpsc::UnboundedReceiver;
use std::time::{SystemTime, UNIX_EPOCH, Duration};
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

pub async fn candles_snapshot(info_client: &InfoClient,coin: &str,time_frame: &str, start: u64, end: u64) -> Vec<Price>{
    let coin = coin;
    let start_timestamp = start;
    let end_timestamp = end;
    let interval = time_frame;

    let mut res: Vec<Price> = Vec::new();
    let vec = info_client
            .candles_snapshot(coin.to_string(), interval.to_string(), start_timestamp, end_timestamp)
            .await
            .unwrap();
    println!("{:?}", vec);
    for candle in vec.iter().take(vec.len() - 1) {
        let h = candle.high.parse::<f32>().unwrap();
        let l = candle.low.parse::<f32>().unwrap();
        let o = candle.open.parse::<f32>().unwrap();
        let c = candle.close.parse::<f32>().unwrap();

        res.push(Price {
            high: h,
            low: l,
            open: o,
            close: c,
    });
    }
    res
}

pub fn tf_to_minutes(tf: &str) -> u64 {
    match tf {
        "1m" => 1,
        "3m" => 3,
        "5m" => 5,
        "15m" => 15,
        "30m" => 30,
        "1h" => 60,
        "2h" => 120,
        "4h" => 240,
        "8h" => 480,
        "12h" => 720,
        "1d" => 1440,
        "3d" => 4320,
        "1w" => 10080,
        "1mo" => 43200,
        _ => panic!("Unsupported timeframe: {}", tf),
    }
}


pub async fn load_candles(info_client: &InfoClient,coin: &str,tf: &str, candle_count: u64) -> Vec<Price> {

    let (end,start) = get_time_now_and_candles_ago(candle_count + 1, tf_to_minutes(tf));

    let price_data = candles_snapshot(info_client, coin, tf, start, end).await;

    price_data
}


