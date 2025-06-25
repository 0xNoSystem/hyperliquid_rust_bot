#![allow(unused_imports)]
#![allow(unused_mut)]
#![allow(unused_variables)]

use log::info;
use std::str::FromStr;
use ethers::types::H160;
use hyperliquid_rust_sdk::{BaseUrl, InfoClient, Message, Subscription, LedgerUpdate};
use tokio::{
    spawn,
    sync::mpsc::unbounded_channel,
    time::{sleep, Duration},
};

#[tokio::main]
async fn main() {
    env_logger::init();
    let mut info_client = InfoClient::new(None, Some(BaseUrl::Testnet)).await.unwrap();
    let user = H160::from_str("0x8b56d7FBC8ad2a90E1C1366CA428efb4b5Bed18F").unwrap();

    let (sender, mut receiver) = unbounded_channel();
    let subscription_id = info_client
        .subscribe(Subscription::UserNonFundingLedgerUpdates{ user }, sender)
        .await
        .unwrap();

    spawn(async move {
        sleep(Duration::from_secs(30000)).await;
        info!("Unsubscribing from user events data");
        info_client.unsubscribe(subscription_id).await.unwrap()
    });

    // Listen for events, including possible liquidations
    while let Some(Message::UserNonFundingLedgerUpdates(update)) = receiver.recv().await {
        
        let dis = update.clone();
        let mut liq_assets: Vec<String> = Vec::new();
        for lp in update.data.non_funding_ledger_updates.into_iter(){
            match lp.delta{
                LedgerUpdate::LedgerLiquidation(pos) =>{
                    liq_assets.extend(pos.liquidated_positions.into_iter().map(|p| p.coin));
                },

                _ => {},
            }

        }

        println!("{:?}", liq_assets);
    

        println!("\nReceived user event data: {dis:?}\n");
        // Here you can match for liquidation-related events if they are included
    }
}
