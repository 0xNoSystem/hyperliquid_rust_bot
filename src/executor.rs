use log::info;
use hyperliquid_rust_sdk::{ExchangeClient,ExchangeDataStatus, ExchangeResponseStatus, MarketOrderParams};

use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

use tokio::{
    time::{sleep, Duration},
};

use crate::trade_setup::{TradeCommand, TradeInfo};

pub struct Executor {
    trade_rv: Option<Receiver<TradeCommand>>,
    info_tx: Option<UnboundedSender<TradeInfo>>,
    asset: String,
    exchange_client: ExchangeClient,
    trade_active: Arc<AtomicBool>,
}
use tokio::sync::mpsc::UnboundedSender;
use flume::Receiver;


impl Executor {

    pub fn new(
        asset: String,
        exchange_client: ExchangeClient,
        
        
    ) -> Self {
        
        Executor{
            trade_rv: None,
            info_tx: None,
            asset,
            exchange_client: exchange_client,
            trade_active: Arc::new(AtomicBool::new(false)),
        }
    }

    pub async fn open_order(&mut self,trade: TradeCommand){
        

        let market_open_params = MarketOrderParams {
            asset: self.asset.as_str(),
            is_buy: trade.is_long,
            sz: trade.size as f64,
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
    async fn close_order(&mut self, trade: TradeCommand)   {

        let market_close_params = MarketOrderParams {
            asset: self.asset.as_str(),
            is_buy: !trade.is_long,
            sz: trade.size as f64,
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



    /*pub async fn market_trade_exec(&mut self,trade: TradeCommand){
        
            if !self.is_active(){
                
                self.trade_active.store(true, Ordering::SeqCst);

                self.open_order(size, is_long).await;
                let _ = sleep(Duration::from_secs(trade.duration)).await;

                self.close_order(size, is_long).await;

                self.trade_active.store(false, Ordering::SeqCst);
        };
    }
     */

    pub fn is_active(&self) -> bool{
        self.trade_active.load(Ordering::SeqCst)
    }
    

    pub fn connect_market(
        &mut self,
        receiver: Receiver<TradeCommand>,
        sender: UnboundedSender<TradeInfo>)
    {
        self.trade_rv = Some(receiver);
        self.info_tx = Some(sender);
    }


    pub fn is_connected(&self) -> bool{
        self.trade_rv.is_some() && self.info_tx.is_some()
    }

    
    pub async fn start(&mut self){

        if self.is_connected(){
            
            while let Ok(trade_signal) = self.trade_rv.as_mut().unwrap().recv_async().await{
                 
             

        }  
   
    }

    }





}
