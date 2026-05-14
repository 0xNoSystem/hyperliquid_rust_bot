use futures::future::join_all;
use hyperliquid_rust_bot::{Error, address};
use hyperliquid_rust_sdk::{BaseUrl, FrontendOpenOrdersResponse, InfoClient};

const TEST_USER_ADDRESS_ENV: &str = "TEST_USER_ADDRESS";

#[tokio::main]
async fn main() -> Result<(), Error> {
    dotenv::dotenv().ok();

    let Ok(user_address) = std::env::var(TEST_USER_ADDRESS_ENV) else {
        println!("{TEST_USER_ADDRESS_ENV} is not set; nothing to test");
        return Ok(());
    };
    let user = address(&user_address)?;

    let info_client = InfoClient::new(None, Some(BaseUrl::Mainnet)).await?;
    let abstraction = info_client.get_user_abstraction(user).await;
    println!("user abstraction fetched: {}", abstraction.is_ok());

    let balances = info_client.user_token_balances(user).await?;
    println!("token balances: {}", balances.balances.len());

    let state = info_client.user_state(user, None).await?;
    println!("asset positions: {}", state.asset_positions.len());

    let dexs: Vec<Option<String>> = info_client
        .perp_dexs()
        .await?
        .into_iter()
        .map(|d| d.map(|d| d.name))
        .collect();

    let futures = dexs
        .iter()
        .map(|d| info_client.frontend_open_orders(user, d.clone()));

    let r: Vec<FrontendOpenOrdersResponse> = join_all(futures)
        .await
        .into_iter()
        .collect::<Result<Vec<_>, Error>>()?
        .into_iter()
        .flatten()
        .collect();

    println!("frontend open orders: {}", r.len());

    Ok(())
}
