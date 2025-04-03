use log::info;
use hyperliquid_rust_sdk::{ExchangeClient,ExchangeDataStatus, ExchangeResponseStatus, MarketOrderParams};

use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

use tokio::{
    time::{sleep, Duration},
};

pub struct Executor {
    asset: String,
    exchange_client: ExchangeClient,
    trade_active: Arc<AtomicBool>,
}

/////   CHANGE NAME TO EXECUTOR AND ADD IN MARKET TO MANAGE TRADES


impl Executor {

    pub fn new(
        asset: String,
        exchange_client: ExchangeClient,
        
        
    ) -> Self {
        
        Executor{
            asset,
            exchange_client: exchange_client,
            trade_active: Arc::new(AtomicBool::new(false)),
        }
    }

    pub async fn open_order(&mut self,size: f32, is_long: bool){

        let market_open_params = MarketOrderParams {
            asset: self.asset.as_str(),
            is_buy: is_long,
            sz: size as f64,
            px: None,
            slippage: Some(0.01), // 1% slippage
            cloid: None,
            wallet: None,
        };

        let response = self.exchange_client
            .market_open(market_open_params)
            .await
            .unwrap();
        info!("Market open order placed: {response:?}");

        let response = match response {
            ExchangeResponseStatus::Ok(exchange_response) => exchange_response,
            ExchangeResponseStatus::Err(e) => panic!("Error with exchange response: {e}"),
        };
        let status = response.data.unwrap().statuses[0].clone();
        match status {
            ExchangeDataStatus::Filled(order) => info!("Order filled: {order:?}"),
            ExchangeDataStatus::Resting(order) => info!("Order resting: {order:?}"),
            _ => panic!("Unexpected status: {status:?}"),
        };
    }
    async fn close_order(&mut self, size: f32, is_long: bool)   {

        let market_close_params = MarketOrderParams {
            asset: self.asset.as_str(),
            is_buy: !is_long,
            sz: size as f64,
            px: None,
            slippage: Some(0.01), // 1% slippage
            cloid: None,
            wallet: None,
        };

        let response = self.exchange_client
            .market_open(market_close_params)
            .await
            .unwrap();
        info!("Market close order placed: {response:?}");

        let response = match response {
            ExchangeResponseStatus::Ok(exchange_response) => exchange_response,
            ExchangeResponseStatus::Err(e) => panic!("Error with exchange response: {e}"),
        };
        let status = response.data.unwrap().statuses[0].clone();
        match status {
            ExchangeDataStatus::Filled(order) => info!("Close order filled: {order:?}"),
            ExchangeDataStatus::Resting(order) => info!("Close order resting: {order:?}"),
            _ => panic!("Unexpected status: {status:?}"),
        };
    }



    pub async fn market_trade_exec(&mut self, size: f32, is_long: bool, time: u64){
        
            if !self.is_active(){
                
                self.trade_active.store(true, Ordering::SeqCst);

                self.open_order(size, is_long).await;
                let _ = sleep(Duration::from_secs(time)).await;

                self.close_order(size, is_long).await;

                self.trade_active.store(false, Ordering::SeqCst);
        };
    }
        
    pub fn is_active(&self) -> bool{
        self.trade_active.load(Ordering::SeqCst)
    }
    
}
