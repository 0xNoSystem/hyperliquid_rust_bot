use crate::Error;
use crate::signal::{IndexId, SignalEngine};
use crate::strategy::{CustomStrategy, Strategy};
use crate::trade_setup::TimeFrame;
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
            "https://api.binance.com/api/v3/klines?symbol={}USDT&interval={}&startTime={}&endTime={}&limit=1000",
            asset,
            tf.as_str(),
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
