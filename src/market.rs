#![allow(unused_imports)]
#![allow(unused_mut)]
#![allow(unused_variables)]
#![allow(dead_code)]
use log::info;
use ethers::signers::LocalWallet;
use hyperliquid_rust_sdk::{Subscription,Message,ExchangeClient, InfoClient, ExchangeDataStatus, ExchangeResponseStatus, MarketOrderParams, BaseUrl};
use crate::trade_setup::{Strategy,Style,Stance, Risk, TradeParams, TradeCommand, PriceData, TradeInfo};
use crate::{MAX_HISTORY, MARKETS};
use crate::{Executor, SignalEngine, IndicatorsConfig, EngineCommand};
use crate::helper::{load_candles, subscribe_candles, get_user_fees};
use kwant::indicators::{Price};

use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

use tokio::{
    sync::mpsc::{channel, Receiver, Sender, unbounded_channel,UnboundedReceiver},
    time::{sleep, Duration},
};
use flume::{bounded, TrySendError};


pub struct Market {
    wallet: LocalWallet,
    public_key: String,
    exchange_client: ExchangeClient,
    info_client: InfoClient,
    pub trade_history: Vec<TradeInfo>,
    pub pnl_history: Vec<f32>,
    trade_active: Arc<AtomicBool>,
    pub trade_params: TradeParams,
    asset: String,
    pub signal_engine: SignalEngine,
    executor: Executor,
    market_rv_tx: (Sender<MarketCommand>, Receiver<MarketCommand>),
}



impl Market{

    pub async fn new(wallet: LocalWallet,public_key: String, asset: String, trade_params: TradeParams, indicators_config: Option<IndicatorsConfig>) -> Result<(Self, Sender<MarketCommand>), String>{
        if !MARKETS.contains(&asset.as_str()){
            return Err("ASSET ISN'T TRADABLE, MARKET CAN'T BE INITILIAZED".to_string());
        }

        let mut info_client = InfoClient::with_reconnect(None, Some(BaseUrl::Mainnet)).await.unwrap();
        let exchange_client = ExchangeClient::new(None, wallet.clone(), Some(BaseUrl::Mainnet), None, None).await.unwrap();
        let exchange_client_exec = ExchangeClient::new(None, wallet.clone(), Some(BaseUrl::Mainnet), None, None).await.unwrap();
        let fees = get_user_fees(&info_client, public_key.clone()).await;
    
        let (market_tx, mut market_rv) = channel::<MarketCommand>(4);


        Ok((Market{
            wallet:wallet.clone(), 
            public_key,
            exchange_client,
            info_client, 
            trade_history: Vec::with_capacity(MAX_HISTORY),
            pnl_history: Vec::with_capacity(MAX_HISTORY),
            trade_active: Arc::new(AtomicBool::new(false)),
            trade_params : trade_params.clone(),
            asset: asset.clone(), 
            signal_engine: SignalEngine::new(indicators_config, trade_params.strategy).await,
            executor: Executor::new(wallet, asset, exchange_client_exec, fees),
            market_rv_tx: (market_tx.clone(), market_rv), 
        }, market_tx.clone()))
    }
    
    async fn init(&mut self) -> Result<(), String>{

        self.trade_params.update_lev(self.trade_params.lev ,&self.exchange_client, self.asset.as_str()).await;
        self.load_engine(300).await?;
        println!("Market initialized for {} {:?}", self.asset, self.trade_params);
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

    pub fn change_strategy(&mut self, strategy: Strategy){

        self.trade_params.strategy = strategy.clone();
        
    }

    pub async fn update_lev(&mut self, lev: u32){
        
    }


    async fn load_engine(&mut self, candle_count: u64) -> Result<(), String>{

        let price_data = load_candles(&self.info_client, self.asset.as_str(), self.trade_params.time_frame.as_str(), candle_count)
        .await?;

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

    pub fn get_trade_history(&self) -> &Vec<TradeInfo>{

        &self.trade_history
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

    pub async fn start(mut self) -> Result<(), String>{
        self.init().await?;
        let mut signal_engine = self.signal_engine;
        let mut executor = self.executor;
        //Setup channels
        let (market_tx, mut market_rv) = self.market_rv_tx;
        let (engine_tx, mut engine_rv) = unbounded_channel::<EngineCommand>();
        let (tx_exec, mut rv_exec) = bounded::<TradeCommand>(0);

        //Subscribe candles
        let mut receiver = subscribe_candles(self.asset.as_str(), self.trade_params.time_frame.as_str()).await;

        //Start engine 
        let trade_tx = tx_exec.clone();
        signal_engine.connect_market(engine_rv, trade_tx);

        //Start exucutor
        let info_tx = market_tx.clone();  
        executor.connect_market(rv_exec, info_tx);
        //main loop
        tokio::spawn(async move {
            signal_engine.start().await;
        });

        tokio::spawn(async move {
            executor.start().await;
        });
        
        //Candle Stream
        let engine_price_tx = engine_tx.clone();
        tokio::spawn(async move {
            while let Some(Message::Candle(candle)) = receiver.recv().await{
                let timestamp = candle.data.time_close;
                let close = candle.data.close.parse::<f32>().ok().unwrap();
                let high = candle.data.high.parse::<f32>().ok().unwrap();
                let low = candle.data.low.parse::<f32>().ok().unwrap();            
                let open = candle.data.open.parse::<f32>().ok().unwrap();
                let price = Price{open,high, low, close};
                let price_data = PriceData{price, time: timestamp};

                let _ = engine_price_tx.send(EngineCommand::UpdatePrice(price_data));
                }
        });

        //listen to edits (exemple: change strategy)
        let engine_update_tx = engine_tx.clone();
        while let Some(cmd) = market_rv.recv().await{
             match cmd {
                   MarketCommand::UpdateLeverage(lev)=>{
                        self.trade_params.update_lev(lev ,&self.exchange_client, self.asset.as_str()).await;
                },

                    MarketCommand::UpdateStrategy(strat)=>{
                        let _ = engine_update_tx.send(EngineCommand::UpdateStrategy(strat));
                    },

                    MarketCommand::UpdateIndicatorsConfig(config)=>{

                        let _ = engine_update_tx.send(EngineCommand::UpdateConfig(config));
                    },
                    
                    MarketCommand::ReceiveTrade(trade_info) =>{
                        self.pnl_history.push(trade_info.pnl);
                        self.trade_history.push(trade_info);
                    },
                    MarketCommand::UpdateTimeFrame(tf)=>{
                        
                        let price_data = load_candles(&self.info_client,
                                                    self.asset.as_str(),
                                                    tf.as_str(),
                                                    500,
                                                        ).await; 
                        if let Ok(price_data) = price_data{
                            self.trade_params.time_frame = tf;
                            let _ = engine_update_tx.send(EngineCommand::Reload(price_data));
                        };
                    },
                    MarketCommand::Close=>{println!("Close {} Market.", self.asset);} 

                 
                }
        };




        Ok(())
    }
}







#[derive(Debug, Clone)]
pub enum MarketCommand{
    UpdateLeverage(u32),
    UpdateStrategy(Strategy),
    UpdateIndicatorsConfig(IndicatorsConfig),
    UpdateTimeFrame(String),
    ReceiveTrade(TradeInfo),
    Close,
}












