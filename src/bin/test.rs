use hyperliquid_rust_bot::{BackTester, TimeFrame};
use reqwest::Client;

#[tokio::main]
async fn main() {
    let bt = BackTester::new();
    bt.fetch_binance_price_data("SOL", TimeFrame::Week, 1761609600000, 1764201600000)
        .await;
    //test_binance_weekly_klines("SOL","1w", 1761609600000 , 1764201600000 ).await;
}
