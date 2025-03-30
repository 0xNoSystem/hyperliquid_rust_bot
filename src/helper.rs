use hyperliquid_rust_sdk::{BaseUrl, InfoClient, Message, Subscription};
use tokio::sync::mpsc::UnboundedReceiver;

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