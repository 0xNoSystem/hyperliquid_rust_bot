
use log::info;
use ethers::signers::LocalWallet;
use hyperliquid_rust_sdk::{Message,ExchangeClient, InfoClient, BaseUrl};
use crate::trade_setup::{TimeFrame,Strategy, TradeParams, TradeCommand, PriceData, TradeInfo};
use crate::{MAX_HISTORY, MARKETS};
use crate::{Executor, SignalEngine, IndicatorsConfig, EngineCommand};
use crate::helper::{load_candles, subscribe_candles, get_user_fees};
use kwant::indicators::{Price};

use tokio::{
    sync::mpsc::{channel, UnboundedSender,Receiver, Sender, unbounded_channel,UnboundedReceiver},
    time::{sleep, Duration},
};
use flume::{bounded, Sender as FlumeSender};


pub struct Market {
    wallet: LocalWallet,
    public_key: String,
    exchange_client: ExchangeClient,
    info_client: InfoClient,
    pub trade_history: Vec<TradeInfo>,
    pnl: f32,
    pub trade_params: TradeParams,
    asset: String,
    pub signal_engine: SignalEngine,
    executor: Executor,
    market_rv: Receiver<MarketCommand>,
    engine_tx: UnboundedSender<EngineCommand>,
    exec_tx: FlumeSender<TradeCommand>, 
}



impl Market{

    pub async fn new(wallet: LocalWallet,public_key: String, asset: String, trade_params: TradeParams, indicators_config: Option<IndicatorsConfig>) -> Result<(Self, Sender<MarketCommand>), String>{
        if !MARKETS.contains(&asset.as_str()){
            return Err("ASSET ISN'T TRADABLE, MARKET CAN'T BE INITILIAZED".to_string());
        }

        let mut info_client = InfoClient::with_reconnect(None, Some(BaseUrl::Mainnet)).await.unwrap();
        let exchange_client = ExchangeClient::new(None, wallet.clone(), Some(BaseUrl::Mainnet), None, None).await.unwrap();

        //fetch user fees %
        let fees = get_user_fees(&info_client, public_key.clone()).await;
        
        //setup channels
        let (market_tx, mut market_rv) = channel::<MarketCommand>(4);
        let (exec_tx, mut rv_exec) = bounded::<TradeCommand>(0);
        let (engine_tx, mut engine_rv) = unbounded_channel::<EngineCommand>();

        Ok((Market{
            wallet:wallet.clone(), 
            public_key,
            exchange_client,
            info_client, 
            trade_history: Vec::with_capacity(MAX_HISTORY),
            pnl: 0_f32,
            trade_params : trade_params.clone(),
            asset: asset.clone(), 
            signal_engine: SignalEngine::new(indicators_config, trade_params.strategy,engine_rv,exec_tx.clone()).await,
            executor: Executor::new(wallet, asset, fees,rv_exec ,market_tx.clone()).await,
            market_rv, 
            engine_tx,
            exec_tx,
        }, market_tx))
    }
    
    async fn init(&mut self) -> Result<(), String>{

        self.trade_params.update_lev(self.trade_params.lev ,&self.exchange_client, self.asset.as_str()).await;
        self.load_engine(300).await?;
        println!("Market initialized for {} {:?}", self.asset, self.trade_params);
        Ok(())
    }



    pub async fn change_time_frame(&mut self, tf: TimeFrame) -> Result<(), String>{
        if tf != self.trade_params.time_frame{
            self.trade_params.time_frame = tf;
            self.signal_engine.reset();

            self.load_engine(300).await?;
        }
        Ok(())
    }

    pub fn change_strategy(&mut self, strategy: Strategy){

        self.trade_params.strategy = strategy.clone();
        
    }
        

    async fn load_engine(&mut self, candle_count: u64) -> Result<(), String>{

        let price_data = load_candles(&self.info_client, self.asset.as_str(), self.trade_params.time_frame, candle_count)
        .await?;

        self.signal_engine.load(&price_data);
        Ok(())

    }
    
}



impl Market{

    pub fn get_trade_history(&self) -> &Vec<TradeInfo>{

        &self.trade_history
    }
    
}


impl Market{

    pub async fn start(mut self) -> Result<(), String>{
        self.init().await?;

        let mut signal_engine = self.signal_engine;
        let mut executor = self.executor;
        
        //Start engine 
        let engine_handle = tokio::spawn(async move {
            signal_engine.start().await;
        });
        //Start exucutor
        let executor_handle = tokio::spawn(async move {
            executor.start().await;
        });
        //Subscribe candles
        let (shutdown_tx, mut receiver) = subscribe_candles(self.asset.as_str(), self.trade_params.time_frame.as_str()).await;



        //Candle Stream
        let engine_price_tx = self.engine_tx.clone();

        let candle_stream_handle = tokio::spawn(async move {
           
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
        let engine_update_tx = self.engine_tx.clone();
        while let Some(cmd) = self.market_rv.recv().await{
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
                        info!("\nMarket received trade result, {:?}\n", &trade_info);
                        self.pnl += trade_info.pnl;
                        self.trade_history.push(trade_info);
                    },
                    MarketCommand::UpdateTimeFrame(tf)=>{
                        
                        let price_data = load_candles(&self.info_client,
                                                    self.asset.as_str(),
                                                    tf,
                                                    500,
                                                        ).await; 
                        if let Ok(price_data) = price_data{
                            self.trade_params.time_frame = tf;
                            let _ = engine_update_tx.send(EngineCommand::Reload(price_data));
                        };
                    },
                    MarketCommand::Pause =>{

                       self.exec_tx.send_async(TradeCommand::Pause).await;  
                    },

                    MarketCommand::Close=>{
                    info!("\nClosing {} Market...\n", self.asset);
                    let _ = shutdown_tx.send(true);
                    let _ = engine_update_tx.send(EngineCommand::Stop);
                    //shutdown Executor
                    info!("\nShutting down executor\n");
                    match self.exec_tx.send(TradeCommand::CancelTrade) {
                        Ok(_) =>{
                            if let Some(cmd) = self.market_rv.recv().await {
                                match cmd {
                                    MarketCommand::ReceiveTrade(trade_info) => {
                                        info!("\nReceived final trade before shutdown: {:?}\n", trade_info);
                                        self.pnl += trade_info.pnl;
                                        self.trade_history.push(trade_info);
                                        break;
                                        },

                                    _ => break,

                                    }}
                            },

                        _ => {
                            log::warn!("Cancel message not sent");
                        },
                        
                        }
                    break;
                    }, 
                };

                };
        
        let _ = engine_handle.await;
        let _ = executor_handle.await;
        let _ = candle_stream_handle.await;
        println!("No. of trade : {}\nPNL: {}",&self.trade_history.len(),&self.pnl);
        Ok(())
    }
}







#[derive(Debug, Clone)]
pub enum MarketCommand{
    UpdateLeverage(u32),
    UpdateStrategy(Strategy),
    UpdateIndicatorsConfig(IndicatorsConfig),
    UpdateTimeFrame(TimeFrame),
    ReceiveTrade(TradeInfo),
    Pause,
    Close,
}












