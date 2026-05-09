use futures::future::join_all;
use hyperliquid_rust_bot::{Error, address};
use hyperliquid_rust_sdk::{BaseUrl, FrontendOpenOrdersResponse, InfoClient};
const WALLET1: &str = "0x8b56d7FBC8ad2a90E1C1366CA428efb4b5Bed18F";

#[tokio::main]
async fn main() {
    let user = address(WALLET1).unwrap();

    let info_client = InfoClient::new(None, Some(BaseUrl::Mainnet)).await.unwrap();
    let abstraction = info_client.get_user_abstraction(user).await;
    let _ = dbg!(abstraction);

    let _ = dbg!(info_client.user_token_balances(user).await);
    let _ = dbg!(info_client.user_state(user, Some("xyz".to_string())).await);

    let dexs: Vec<Option<String>> = info_client
        .perp_dexs()
        .await
        .unwrap()
        .into_iter()
        .map(|d| d.map(|d| d.name))
        .collect();

    let futures = dexs
        .iter()
        .map(|d| info_client.frontend_open_orders(user, d.clone()));

    let r: Vec<FrontendOpenOrdersResponse> = join_all(futures)
        .await
        .into_iter()
        .collect::<Result<Vec<_>, Error>>()
        .unwrap()
        .into_iter()
        .flatten()
        .collect();

    dbg!(r);
}
