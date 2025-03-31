use log::info;
use ethers::signers::LocalWallet;
use hyperliquid_rust_sdk::{ExchangeClient, InfoClient, ExchangeDataStatus, ExchangeResponseStatus, MarketOrderParams,};
use crate::trade_setup::{Strategy, Risk, TradeParams};

use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

use tokio::{
    time::{sleep, Duration},
};



pub struct Market {
    wallet: LocalWallet,
    public_key: String,
    exchange_client: ExchangeClient,
    info_client: InfoClient,
    pub pnl_history: Vec<f32>,
    trade_active: Arc<AtomicBool>,
    pub trade_params: TradeParams,
}




impl Market {

    pub fn new(
        wallet: LocalWallet,
        public_key: String,
        info_client: InfoClient,
        exchange_client: ExchangeClient,
        trade_params: TradeParams,
        
    ) -> Self {
        
        Market{
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
        println!("\nBot init: SUCCESS\n");
        println!("Trade settings:\n\n{}", &self.trade_params);
    }

    pub async fn update_lev(&mut self, lev: u32){
        
        self.trade_params.update_lev(lev, &self.exchange_client).await;
    }

    pub fn get_pnl_history(&self) -> &Vec<f32>{

        &self.pnl_history
    } 


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



    pub async fn market_trade_exec(&mut self, size: f32, signal: Option<bool>){
        
        if let Some(pos)  = signal{
                if !self.is_active(){
                    let direction = if pos { "LONG" } else { "SHORT" };
                    self.trade_active.store(true, Ordering::SeqCst);

                    self.open_order(size, pos).await;
                    
                    println!("--------------------BOT:Trade opened ({})", direction);

                    let _ = sleep(Duration::from_secs(self.trade_params.trade_time)).await;

                    self.close_order(size, pos).await;
                    println!("--------------------BOT:Closed trade ({})", direction);

                    let pnl = self.get_last_pnl().await;
                    self.pnl_history.push(pnl);
                    println!("--------------------PNL: {}\n--------------------Session PNL: {}", pnl, self.get_session_pnl());   
                    self.trade_active.store(false, Ordering::SeqCst);
        };
        };
    }
        
    pub async fn get_signal(&self, rsi: f32) -> Option<bool>{
        
        let thresh = self.get_rsi().await;
        match self.trade_params.strategy{
            Strategy::Bull => {
                if  rsi < thresh.0{
                    return Some(true);
                }else{
                    return None;
                }
            },

            Strategy::Bear =>   {
                if rsi > thresh.1{
                    return Some(false);
                }else{
                    return None;
                }
            }

            Strategy::Neutral => {
                if rsi < thresh.0{
                    return Some(true);
                }else if rsi > thresh.1{
                    return Some(false);
                }else{
                    return None;
                }
            }
        }

    }

    pub async fn get_rsi(&self) -> (f32, f32){
        match self.trade_params.risk{
            Risk::Low => (25.0, 77.0),
            Risk::Medium => (30.0, 70.0),
            Risk::High => (35.0, 67.0),
        }
    }


    async fn get_last_pnl(&self) -> f32{

        let user =   self.public_key.parse().unwrap();

        let fills = self.info_client.user_fills(user).await.unwrap();
        
        let close_fee = fills[0].fee.parse::<f32>().unwrap();
        let open_fee = fills[1].fee.parse::<f32>().unwrap();

        let pnl = fills[0].closed_pnl.parse::<f32>().unwrap();

        return pnl - open_fee - close_fee;
    }

    fn get_session_pnl(&self) -> f32{

        self.pnl_history.iter().sum()
    }

    pub fn is_active(&self) -> bool{
        self.trade_active.load(Ordering::SeqCst)
    }

}

#[derive(Debug)]
pub enum MarketCommand {
    ExecuteTrade { size: f32, rsi: f32 },
}

