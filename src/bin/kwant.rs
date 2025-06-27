#![allow(unused_imports)]
#![allow(unused_mut)]
#![allow(unused_variables)]

use log::info;
use std::collections::HashMap;
use std::str::FromStr;
use ethers::types::H160;
use hyperliquid_rust_sdk::{BaseUrl, InfoClient, Message, Subscription, TradeInfo};
use tokio::{
    spawn,
    sync::mpsc::unbounded_channel,
    time::{sleep, Duration},
};

use hyperliquid_rust_bot::{
    LiquidationFillInfo,
};

#[tokio::main]
async fn main() {
    env_logger::init();
    let mut info_client = InfoClient::new(None, Some(BaseUrl::Testnet)).await.unwrap();
    let user = H160::from_str("0x8b56d7FBC8ad2a90E1C1366CA428efb4b5Bed18F").unwrap();

    let (sender, mut receiver) = unbounded_channel();
    let subscription_id = info_client
        .subscribe(Subscription::UserFills{ user }, sender)
        .await
        .unwrap();

    spawn(async move {
        sleep(Duration::from_secs(30000)).await;
        info!("Unsubscribing from user events data");
        info_client.unsubscribe(subscription_id).await.unwrap()
    });

    // Listen for events, including possible liquidations
    while let Some(Message::UserFills(update)) = receiver.recv().await {

        if update.data.is_snapshot.is_some(){
            continue;
        }

        let mut liq_map: HashMap<String, Vec<TradeInfo>> = HashMap::new(); 

        for trade in update.data.fills.into_iter(){
            if trade.liquidation.is_some(){
                liq_map
                    .entry(trade.coin.clone())
                    .or_insert_with(Vec::new)
                    .push(trade);
            }
        }
        println!("\nTRADES  |||||||||| {:?}\n\n", liq_map);
        
        for (coin, fills) in liq_map.into_iter(){
            let to_send = LiquidationFillInfo::from(fills);
            println!("\nTRADE FILL INFO: {:?}", to_send);
        }

}

}




