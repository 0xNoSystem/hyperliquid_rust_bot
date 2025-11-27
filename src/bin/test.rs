use reqwest::Client;

#[tokio::main]
async fn main(){

    test_binance_weekly_klines("SOL","1w", 1761609600000 , 1764201600000 ).await;
}

async fn test_binance_weekly_klines(asset: &str, tf: &str, start_ms: u64, end_ms: u64) {

    let url = format!(
        "https://api.binance.com/api/v3/klines?symbol={}USDT&interval={}&startTime={}&endTime={}&limit=1000",
        asset, tf, start_ms, end_ms
    );

    let client = Client::new();
    let res = client.get(&url).send().await;

    match res {
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_else(|_| "failed to read body".to_string());
            println!("STATUS: {}", status);
            println!("BODY: {}", body);
        }
        Err(e) => {
            println!("REQUEST ERROR: {:?}", e);
        }
    }
}

