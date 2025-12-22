/*
use hyperliquid_rust_bot::Error;
use hyperliquid_rust_bot::{TimeFrame, Strategy, get_time_now_and_candles_ago};
use reqwest::Client;
use serde::Deserialize;
use tokio::time::{Duration, sleep};

pub struct BackTester {
    client: Client,
}

#[derive(Debug, Clone, Deserialize)]
struct BackTestRequest {
    asset: String,
    lev: u64,
    margin: u64,
    startTime: u64,
    endTime: u64,
    strategy: Strategy,
}

impl BackTester {
    pub fn new() -> Self {
        BackTester {
            client: Client::new(),
        }
    }

    pub async fn fetch_binance_price_data(
        &self,
        asset: &str,
        tf: TimeFrame,
        startTime: u64,
        endTime: u64,
    ) {
        let url = format!(
            "https://api.kucoin.com/api/v1/market/candles?symbol={}-USDT&type=1hour&startAt={}&endAt={}",
            asset,
            startTime,
            endTime
        );

        let res = self.client.get(&url).send().await;

        match res {
            Ok(response) => {
                println!("{:?}", response.text().await.unwrap());
            }

            Err(e) => {
                println!("REQUEST ERROR: {:?}", e);
            }
        }
    }

    pub fn run(
        BackTestRequest {
            asset,
            lev,
            margin,
            startTime,
            endTime,
            strategy,
        }: BackTestRequest,
    ) -> Result<(), Error> {
        if endTime >= startTime {
            return Err(Error::BacktestError(format!(
                "Invalid time slice <start> should be less than <end>"
            )));
        }

        Ok(())
    }
}
#[tokio::main]
*/
fn main() {
    /*
    let bt = BackTester::new();

    let (start, end) = get_time_now_and_candles_ago(20, TimeFrame::Hour1);
    let start = start / 1000;
    let end = end / 1000;
    bt.fetch_binance_price_data("MON", TimeFrame::Day1, start, end).await;
    */
}
