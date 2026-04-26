use hyperliquid_rust_sdk::{BaseUrl, InfoClient};

#[tokio::main]
async fn main() {
    let info = InfoClient::new(None, Some(BaseUrl::Mainnet)).await.unwrap();

    let dexs = info.all_perp_metas().await.unwrap();
    println!("{dexs:#?}");

    let all_assets = info.all_perp_metas().await.unwrap();
    let asset_names: Vec<String> = all_assets.into_iter().map(|a| a.name).collect();
    dbg!(asset_names);
}
