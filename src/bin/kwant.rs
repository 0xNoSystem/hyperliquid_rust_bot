#![allow(unused_imports)]
#![allow(unused_mut)]
#![allow(unused_variables)]

use ethers::types::H160;
use hyperliquid_rust_sdk::{BaseUrl, InfoClient};
use log::info;

const ADDRESS: &str = "0x8b56d7FBC8ad2a90E1C1366CA428efb4b5Bed18F";

async fn user_state_example(info_client: &InfoClient) {
    let user = address();

    info!(
        "User state data for {user}: {:?}",
        info_client.user_state(user).await.unwrap()
    );
}

fn address() -> H160 {
    ADDRESS.to_string().parse().unwrap()
}

#[tokio::main]
async fn main() {
    env_logger::init();
    let mut info_client = InfoClient::new(None, Some(BaseUrl::Mainnet)).await.unwrap();

    let info =  info_client.user_state(address()).await.unwrap();
    
    let res =  info.margin_summary.account_value
        .parse::<f64>().unwrap();

    let upnl: f64 = info.asset_positions.into_iter().filter_map(|p|{
        let u = p.position.unrealized_pnl.parse::<f64>().ok()?;
        let f =  p.position.cum_funding.since_open.parse::<f64>().ok()?;
        Some(u - f)
    }).sum();

    println!("{}", upnl);
}
