use log::info;
use ethers::types::H160;
use ethers::signers::LocalWallet;
use hyperliquid_rust_sdk::{ExchangeClient, InfoClient, ExchangeDataStatus, ExchangeResponseStatus, MarketOrderParams,};
use indicators::rsi2::Rsi;
use crate::trade_setup::{Strategy, Risk, TradeParams};

use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

use tokio::{
    spawn,
    sync::mpsc::{unbounded_channel},
    time::{sleep, Duration},
};



pub struct Bot {
    wallet: LocalWallet,
    public_key: String,
    exchange_client: ExchangeClient,
    info_client: InfoClient,
    pub pnl_history: Vec<f32>,
    trade_active: Arc<AtomicBool>,
    pub trade_params: TradeParams,
}




impl Bot {

    pub fn new(
        wallet: LocalWallet,
        public_key: String,
        info_client: InfoClient,
        exchange_client: ExchangeClient,
        trade_params: TradeParams,
        
    ) -> Self {
        
        Bot{
            wallet: wallet,
            public_key: public_key,
            info_client: info_client,
            exchange_client: exchange_client,
            trade_params: trade_params,
            pnl_history: Vec::new(),
            trade_active: Arc::new(AtomicBool::new(false)),
        }
    }

    pub async fn init(&mut self){

        self.update_lev(self.trade_params.lev).await;
    }

    pub async fn update_lev(&mut self, lev: u32){
        
        self.trade_params.update_lev(lev, &self.exchange_client).await;
    }

    pub fn get_pnl_history(&self) -> &Vec<f32>{

        &self.pnl_history
    }
    
    }



impl Bot{

    pub async fn open_order(&mut self, size: f32, is_long: bool){

        let market_open_params = MarketOrderParams {
            asset: self.trade_params.asset.as_str(),
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
            asset: self.trade_params.asset.as_str(),
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




    pub async fn trade_exec(&mut self, size: f32, is_long: bool){

            self.trade_active.store(true, Ordering::SeqCst);

            self.open_order(size, is_long).await;

            let _ = sleep(Duration::from_secs(self.trade_params.trade_time)).await;

            self.close_order(size, is_long).await;

            self.trade_active.store(false, Ordering::SeqCst);
        
    }




    async fn get_last_pnl(&self, info_client: &InfoClient) -> f32{
        println!("HEY");
        let user =   self.public_key.parse().unwrap();

        let fills = info_client.user_fills(user).await.unwrap();
    
        let fee = fills[0].fee.parse::<f32>().unwrap();

        let pnl = fills[0].closed_pnl.parse::<f32>().unwrap();

        return pnl - fee
}

    pub fn is_active(&self) -> bool{
        self.trade_active.load(Ordering::SeqCst)
    }

}