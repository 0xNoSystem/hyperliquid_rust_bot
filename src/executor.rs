use log::info;
use hyperliquid_rust_sdk::{ExchangeClient,ExchangeDataStatus, ExchangeResponseStatus, MarketOrderParams};

use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

use tokio::{
    time::{sleep, Duration},
};

use crate::trade_setup::{TradeCommand, TradeFillInfo, TradeInfo};

pub struct Executor {
    trade_rv: Option<Receiver<TradeCommand>>,
    info_tx: Option<UnboundedSender<TradeInfo>>,
    asset: String,
    exchange_client: ExchangeClient,
    trade_active: Arc<AtomicBool>,
    fees: (f32, f32),
}
use tokio::sync::mpsc::UnboundedSender;
use flume::Receiver;


impl Executor {

    pub fn new(
        asset: String,
        exchange_client: ExchangeClient,
        fees: (f32, f32),
        
    ) -> Self {
        
        Executor{
            trade_rv: None,
            info_tx: None,
            asset,
            exchange_client: exchange_client,
            trade_active: Arc::new(AtomicBool::new(false)),
            fees,
        }
    }

    pub async fn open_order(&mut self,trade: TradeCommand) -> Option<TradeFillInfo>{
        
        self.trade_active.store(true, Ordering::SeqCst);

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

         match status{
            
            ExchangeDataStatus::Filled(ref order) =>  {
            
                println!("Open order filled: {order:?}");
                let sz: f32 = order.total_sz.parse::<f32>().unwrap();
                let price: f32 = order.avg_px.parse::<f32>().unwrap(); 
                let fill_info = TradeFillInfo{fill_type: "Open".to_string(),sz, price, oid: order.oid};
            
                return Some(fill_info);
            },

            _ => None,
            }


    }
    pub async fn close_order(&mut self, trade: TradeCommand) -> Option<TradeFillInfo>   {

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
        match status{

            ExchangeDataStatus::Filled(ref order) =>  {

                println!("Close order filled: {order:?}");
                let sz: f32 = order.total_sz.parse::<f32>().unwrap();
                let price: f32 = order.avg_px.parse::<f32>().unwrap(); 
                let fill_info = TradeFillInfo{fill_type: "Close".to_string(),sz, price, oid: order.oid};
                self.trade_active.store(false, Ordering::SeqCst);
                return Some(fill_info);
            },

            _ => None,
    }
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


    fn calculate_pnl(&self,is_long: bool, trade_fill_open: &TradeFillInfo, trade_fill_close: &TradeFillInfo) -> (f32, f32){
        let fee_open = trade_fill_open.sz * trade_fill_open.price * self.fees.1;
        let fee_close = trade_fill_close.sz * trade_fill_close.price * self.fees.1;
        
        let pnl = if is_long{
            trade_fill_close.sz * (trade_fill_close.price - trade_fill_open.price) - fee_open - fee_close
        }else{
            trade_fill_close.sz * (trade_fill_open.price - trade_fill_close.price) - fee_open - fee_close
        };

        (fee_open + fee_close, pnl)
    }

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
        println!("EXECUTOR STARTED");
        if self.is_connected(){
            let info_sender = self.info_tx.clone().unwrap();
            while let Ok(trade_signal) = self.trade_rv.as_mut().unwrap().recv_async().await{
                 
        
                let trade_fill_open = self.open_order(trade_signal).await.unwrap();
                let _ = sleep(Duration::from_secs(trade_signal.duration)).await;

                let trade_fill_close = self.close_order(trade_signal).await.unwrap();
        
                let (fees, pnl) = self.calculate_pnl(trade_signal.is_long, &trade_fill_open, &trade_fill_close);

                let trade_info = TradeInfo{
                    open: trade_fill_open.price,
                    close: trade_fill_close.price,
                    pnl,
                    fee: fees,
                    is_long: trade_signal.is_long,
                    duration: trade_signal.duration,
                    oid: (trade_fill_open.oid, trade_fill_close.oid)
                };

                let _ = info_sender.send(trade_info);
        }  
   
    }

    }




}