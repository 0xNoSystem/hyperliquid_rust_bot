use log::info;
use hyperliquid_rust_sdk::{ExchangeClient,ExchangeDataStatus, ExchangeResponseStatus, MarketOrderParams, BaseUrl};
use ethers::signers::LocalWallet;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

use tokio::{
    sync::mpsc::{Sender},
    time::{sleep, Duration},
};

use crate::trade_setup::{TradeCommand, TradeFillInfo, TradeInfo};
use flume::{Receiver};
use crate::market::MarketCommand;

pub struct Executor {
    wallet: LocalWallet,
    trade_rv: Receiver<TradeCommand>,
    market_tx: Sender<MarketCommand>,
    asset: String,
    exchange_client: ExchangeClient,
    trade_active: Arc<AtomicBool>,
    fees: (f32, f32),
    open_position: Option<TradeFillInfo>,
}



impl Executor {

    pub async fn new(
        wallet: LocalWallet,
        asset: String,
        fees: (f32, f32),
        trade_rv: Receiver<TradeCommand>, 
        market_tx: Sender<MarketCommand>,
    ) -> Self {
        
        let exchange_client = ExchangeClient::new(None, wallet.clone(), Some(BaseUrl::Mainnet), None, None).await.unwrap();
        Executor{
            wallet,
            trade_rv,
            market_tx,
            asset,
            exchange_client,
            trade_active: Arc::new(AtomicBool::new(false)),
            fees,
            open_position: None,
        }
    }

    async fn try_trade(&self, params: MarketOrderParams<'_>) -> Result<ExchangeDataStatus, String>{

        let response = self.exchange_client
            .market_open(params)
            .await
            .map_err(|e| format!("Transport failure, {}",e))?;

        info!("Market order placed: {response:?}");

        let response = match response {
            ExchangeResponseStatus::Ok(exchange_response) => exchange_response,
            ExchangeResponseStatus::Err(e) => {
                return Err(format!("Exchange Error: Couldn't execute trade => {}",e));
         }
        };
     
        let status = response.data.unwrap().statuses[0].clone();

        Ok(status)

    }
    pub async fn open_order(&self,size: f32, is_long: bool) -> Result<TradeFillInfo, String>{
        
        let market_open_params = MarketOrderParams {
            asset: self.asset.as_str(),
            is_buy: is_long,
            sz: size as f64,
            px: None,
            slippage: Some(0.01), // 1% slippage
            cloid: None,
            wallet: None,
        };
        
        let status = self.try_trade(market_open_params).await?;

         match status{
            
            ExchangeDataStatus::Filled(ref order) =>  {
            
                println!("Open order filled: {order:?}");
                let sz: f32 = order.total_sz.parse::<f32>().unwrap();
                let price: f32 = order.avg_px.parse::<f32>().unwrap(); 
                let fill_info = TradeFillInfo{fill_type: "Open".to_string(),sz, price, oid: order.oid, is_long};
                
                Ok(fill_info)
            },

            _ => Err("Open order not filled".to_string()),
            }


    }
    pub async fn close_order(&self, size: f32, is_long: bool) -> Result<TradeFillInfo, String>   {

        if !self.is_active(){
            return Err("Trade not active".to_string());
        }

        let market_close_params = MarketOrderParams {
            asset: self.asset.as_str(),
            is_buy: !is_long,
            sz: size as f64,
            px: None,
            slippage: Some(0.01), // 1% slippage
            cloid: None,
            wallet: None,
        };

        
        let status = self.try_trade(market_close_params).await?;
        match status{

            ExchangeDataStatus::Filled(ref order) =>  {

                println!("Close order filled: {order:?}");
                let sz: f32 = order.total_sz.parse::<f32>().unwrap();
                let price: f32 = order.avg_px.parse::<f32>().unwrap(); 
                let fill_info = TradeFillInfo{fill_type: "Close".to_string(),sz, price, oid: order.oid, is_long};
                return Ok(fill_info);
            },

            _ => Err("Close order not filled".to_string()),
    }
    }



    pub async fn market_trade_exec(&mut self, size: f32, is_long: bool, duration: u64) -> Result<TradeInfo, String>{
        
        let trade_fill_open = self.open_order(size, is_long).await?;
        self.open_position = Some(trade_fill_open.clone());
        let _ = sleep(Duration::from_secs(duration)).await;


        let trade_fill_close = self.close_order(trade_fill_open.sz, trade_fill_open.is_long).await?;
        
        let (fees, pnl) = Self::calculate_pnl(&self.fees, is_long, &trade_fill_open, &trade_fill_close);

        return Ok(TradeInfo{
            open: trade_fill_open.price,
            close: trade_fill_close.price,
            pnl,
            fee: fees,
            is_long,
            duration: Some(duration),
            oid: (trade_fill_open.oid, trade_fill_close.oid)
        });
}
        fn get_trade_info(open: TradeFillInfo, close: TradeFillInfo, fees: &(f32, f32)) -> TradeInfo{
            let is_long = open.is_long;
            let (fee, pnl) = Self::calculate_pnl(fees,is_long, &open, &close);

            TradeInfo{
                open: open.price,
                close: close.price,
                pnl,
                fee,
                is_long, 
                duration: None,
                oid: (open.oid, close.oid),
            }
        }


     


    fn calculate_pnl(fees: &(f32, f32) ,is_long: bool, trade_fill_open: &TradeFillInfo, trade_fill_close: &TradeFillInfo) -> (f32, f32){
        let fee_open = trade_fill_open.sz * trade_fill_open.price * fees.1;
        let fee_close = trade_fill_close.sz * trade_fill_close.price * fees.1;
        
        let pnl = if is_long{
            trade_fill_close.sz * (trade_fill_close.price - trade_fill_open.price) - fee_open - fee_close
        }else{
            trade_fill_close.sz * (trade_fill_open.price - trade_fill_close.price) - fee_open - fee_close
        };

        (fee_open + fee_close, pnl)
    }

    pub fn is_active(&self) -> bool{
        self.trade_active.load(Ordering::Relaxed)
    }
    

    
    pub async fn start(mut self){
        println!("EXECUTOR STARTED");
             
            let info_sender = self.market_tx.clone();
            while let Ok(cmd) = self.trade_rv.recv_async().await{
                match cmd{
                        TradeCommand::ExecuteTrade {size, is_long, duration} => {
                        if !self.is_active(){ 
                                self.trade_active.store(true, Ordering::Relaxed);
                                let trade_info = self.market_trade_exec(size, is_long, duration).await;
                                if let Ok(trade_info) = trade_info{ 
                                    let _ = info_sender.send(MarketCommand::ReceiveTrade(trade_info)).await;
                                    };

                                self.trade_active.store(false, Ordering::Relaxed);
                            };
                        },

                    TradeCommand::OpenTrade{size, is_long}=> {
                        
                        if !self.is_active(){
                            println!("Open trade command received");
                            self.trade_active.store(true, Ordering::Relaxed);
                            let trade_fill = self.open_order(size, is_long).await;

                            if let Ok(trade) = trade_fill{
                            info!("Trade Opened: {:?}", trade.clone());
                                self.open_position = Some(trade);
                            }


                    }
                },

                    TradeCommand::CloseTrade{size, is_long} => {
                        
                        if self.is_active(){
                            let trade_fill = self.close_order(size,is_long).await;
                            if let (Ok(fill), Some(open)) = (trade_fill, self.open_position.take()){
                                let trade_info = Self::get_trade_info(
                                                        open,
                                                        fill,
                                                        &self.fees);
                                let _ = info_sender.send(MarketCommand::ReceiveTrade(trade_info)).await;
                                self.open_position = None;
                                info!("Trade Closed: {:?}", trade_info);
                            }; 
                            self.trade_active.store(false, Ordering::Relaxed);                  
                    }
                },
 
                    TradeCommand::CancelTrade => {
                        if self.is_active(){
                            if let Some(ref pos) = self.open_position.take(){
                                info!("Shutting down executor");
                                let trade_fill = self.close_order(pos.sz, pos.is_long).await;
                                self.trade_active.store(false, Ordering::Relaxed);
                        };};

                        return;

                    },

                    _ => {println!("Command not ready");},
        }


    }}


}

