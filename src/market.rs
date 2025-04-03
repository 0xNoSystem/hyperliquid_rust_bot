#![allow(unused_imports)]
#![allow(unused_mut)]
#![allow(unused_variables)]
#![allow(dead_code)]
use log::info;
use ethers::signers::LocalWallet;
use hyperliquid_rust_sdk::{ExchangeClient, InfoClient, ExchangeDataStatus, ExchangeResponseStatus, MarketOrderParams, BaseUrl};
use crate::trade_setup::{Strategy, Risk, TradeParams, TradeCommand};
use crate::{MAX_HISTORY, MARKETS};
use crate::{Executor, SignalEngine, IndicatorsConfig};
use crate::helper::{load_candles, subscribe_candles};
use kwant::indicators::{Price};

use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

use tokio::{
    sync::mpsc::{unbounded_channel,UnboundedReceiver},
    time::{sleep, Duration},
};
use flume::{bounded, TrySendError};


pub struct Market {
    wallet: LocalWallet,
    public_key: String,
    exchange_client: ExchangeClient,
    info_client: InfoClient,
    pub pnl_history: Vec<f32>,
    trade_active: Arc<AtomicBool>,
    pub trade_params: TradeParams,
    asset: String,
    pub signal_engine: SignalEngine,
    executor: Executor
}



impl Market{

    pub async fn new(wallet: LocalWallet,public_key: String, asset: String, trade_params: TradeParams, indicators_config: Option<IndicatorsConfig>) -> Result<Self, String>{
        if !MARKETS.contains(&asset.as_str()){
            return Err("ASSET ISN'T TRADABLE, MARKET CAN'T BE INITILIAZED".to_string());
        }

        let mut info_client = InfoClient::with_reconnect(None, Some(BaseUrl::Mainnet)).await.unwrap();
        let exchange_client = ExchangeClient::new(None, wallet.clone(), Some(BaseUrl::Mainnet), None, None).await.unwrap();
        let exchange_client_exec = ExchangeClient::new(None, wallet.clone(), Some(BaseUrl::Mainnet), None, None).await.unwrap();

        Ok(Market{
            wallet, 
            public_key,
            exchange_client,
            info_client, 
            pnl_history: Vec::with_capacity(MAX_HISTORY),
            trade_active: Arc::new(AtomicBool::new(false)),
            trade_params : trade_params.clone(),
            asset: asset.clone(), 
            signal_engine: SignalEngine::new(indicators_config, trade_params.strategy).await,
            executor: Executor::new(asset, exchange_client_exec),
        })
    }
    
    async fn init(&mut self) -> Result<(), String>{

        self.update_lev(self.trade_params.lev).await;
        self.load_engine(300).await?;
        Ok(())
    }



    pub async fn change_time_frame(&mut self, tf: &str) -> Result<(), String>{
        if tf != self.trade_params.time_frame{
            self.trade_params.time_frame = tf.to_string();
            self.signal_engine.reset();

            self.load_engine(300).await?;
        }
        Ok(())
    }

    pub async fn update_lev(&mut self, lev: u32){
        
        self.trade_params.update_lev(lev, &self.exchange_client, self.asset.clone()).await;
    }


    async fn load_engine(&mut self, candle_count: u64) -> Result<(), String>{

        let price_data = load_candles(&self.info_client, self.asset.as_str(), self.trade_params.time_frame.as_str(), candle_count).await?;

        self.signal_engine.load(&price_data);
        Ok(())

    }

    pub fn is_active(&self) -> bool{
        self.trade_active.load(Ordering::SeqCst)
    }
}



impl Market{

    pub fn get_pnl_history(&self) -> &Vec<f32>{

        &self.pnl_history
    } 

    pub async fn get_last_pnl(&self) -> Result<f32, String>{

        let user = self.public_key.parse().unwrap();

        let fills = self.info_client.user_fills(user).await.unwrap();
        
        if !fills.is_empty(){
            let close_fee = fills[0].fee.parse::<f32>().unwrap();
            let open_fee = fills[1].fee.parse::<f32>().unwrap();
            let pnl = fills[0].closed_pnl.parse::<f32>().unwrap();

            return Ok(pnl - open_fee - close_fee);
        }else{
            return Err(String::from("No previous fills"));
        }
    
        
    }

    fn get_session_pnl(&self) -> f32{

        self.pnl_history.iter().sum()
    }
}


impl Market{

    pub async fn start(&mut self) -> Result<(), String>{
        self.init().await?;

        //Setup channels
        let (tx_exec, mut rv_exec) = bounded::<TradeCommand>(0);
        let (tx_signal, mut rv_signal) = unbounded_channel::<Price>();

        //Subscribe candles
        let mut recv = subscribe_candles(self.asset.as_str(), self.trade_params.time_frame.as_str());

        //Start engine
        

        //Start exucutor


        //main loop

        Ok(())
    }
}