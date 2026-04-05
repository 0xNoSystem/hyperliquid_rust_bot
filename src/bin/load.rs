use hyperliquid_rust_sdk::{BaseUrl, InfoClient};

#[tokio::main]
async fn main() {
    let info = InfoClient::new(None, Some(BaseUrl::Mainnet)).await.unwrap();

    let dexs = info.all_perp_metas().await.unwrap();
    println!("{dexs:#?}");

    let all_assets = info.all_perp_metas().await.unwrap();
    println!("Total assets across all dexes: {}", all_assets.len());
}
