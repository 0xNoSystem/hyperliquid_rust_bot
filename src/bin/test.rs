use alloy::primitives::address;
use tokio::{
    spawn,
    sync::mpsc::unbounded_channel,
    time::{Duration, sleep},
};

use alloy::signers::local::PrivateKeySigner;
use dotenv::dotenv;
use hyperliquid_rust_sdk::{
    BaseUrl, ClientCancelRequest, ClientLimit, ClientOrder, ClientOrderRequest, ExchangeClient,
    ExchangeDataStatus, ExchangeResponseStatus, InfoClient, Message, Subscription,
};
use log::info;

#[tokio::main]
async fn main() {
    dotenv::from_filename("testnet").ok();
    env_logger::init();
    // Key was randomly generated for testing and shouldn't be used with any real funds
    let wallet = std::env::var("PRIVATE_KEY")
        .expect("Error fetching PRIVATE_KEY")
        .parse()
        .unwrap();

    let exchange_client = ExchangeClient::new(None, wallet, Some(BaseUrl::Testnet), None, None)
        .await
        .unwrap();

    let order = ClientOrderRequest {
        asset: "ETH".to_string(),
        is_buy: false,
        reduce_only: false,
        limit_px: 3119.0,
        sz: 0.1,
        cloid: None,
        order_type: ClientOrder::Limit(ClientLimit {
            tif: "Gtc".to_string(),
        }),
    };

    let mut info_client = InfoClient::new(None, Some(BaseUrl::Testnet)).await.unwrap();
    let user = address!("0x8b56d7FBC8ad2a90E1C1366CA428efb4b5Bed18F");

    let (sender, mut receiver) = unbounded_channel();
    let subscription_id = info_client
        .subscribe(Subscription::OrderUpdates { user }, sender)
        .await
        .unwrap();

    let handle = spawn(async move {
        sleep(Duration::from_secs(3000)).await;
        println!("Unsubscribing from order updates data");
        info_client.unsubscribe(subscription_id).await.unwrap()
    });

    // this loop ends when we unsubscribe
    spawn(async move {
        while let Some(Message::OrderUpdates(order_updates)) = receiver.recv().await {
            println!("Received order update data: {order_updates:?}");
        }
    });

    let response = exchange_client.order(order, None).await.unwrap();
    info!("Order placed: {response:?}");

    let response = match response {
        ExchangeResponseStatus::Ok(exchange_response) => exchange_response,
        ExchangeResponseStatus::Err(e) => panic!("error with exchange response: {e}"),
    };
    let status = dbg!(response.data.unwrap().statuses[0].clone());
    let oid = match status {
        ExchangeDataStatus::Filled(order) => order.oid,
        ExchangeDataStatus::Resting(order) => order.oid,
        _ => panic!("Error: {status:?}"),
    };

    let cancel = ClientCancelRequest {
        asset: "ETH".to_string(),
        oid,
    };

    handle.await.unwrap();
}
