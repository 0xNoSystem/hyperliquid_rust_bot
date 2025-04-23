use log::info;
use ethers::signers::LocalWallet;
use hyperliquid_rust_sdk::{AssetMeta, Message,ExchangeClient, InfoClient, BaseUrl};
use crate::trade_setup::{TimeFrame,TradeParams, TradeCommand, PriceData, TradeInfo};
use crate::strategy::Strategy;
use crate::{MAX_HISTORY, MARKETS};
use crate::{Wallet, Executor, SignalEngine, IndicatorsConfig, EngineCommand};
use crate::signal::ExecParam;
use crate::helper::{get_asset, load_candles, subscribe_candles};
use kwant::indicators::{Price};

use tokio::{
    sync::mpsc::{channel, UnboundedSender,Receiver, Sender, unbounded_channel,UnboundedReceiver},
    time::{sleep, Duration},
};
use flume::{bounded, Sender as FlumeSender};


pub struct Market {
    exchange_client: ExchangeClient,
    info_client: InfoClient,
    pub margin: f32,
    pub trade_history: Vec<TradeInfo>,
    pnl: f32,
    pub trade_params: TradeParams,
    asset: AssetMeta,
    pub signal_engine: SignalEngine,
    executor: Executor,
    market_rv: Receiver<MarketCommand>,
    engine_tx: UnboundedSender<EngineCommand>,
    exec_tx: FlumeSender<TradeCommand>, 
    url: BaseUrl,
}



impl Market{

    pub async fn new(wallet: Wallet,
                    asset: String,
                    mut trade_params: TradeParams,
                    indicators_config: Option<IndicatorsConfig>
    ) -> Result<(Self, Sender<MarketCommand>), String>{

        if !MARKETS.contains(&asset.as_str().trim()){
            return Err("ASSET ISN'T TRADABLE, MARKET CAN'T BE INITILIAZED".to_string());
        }

        let mut info_client = InfoClient::with_reconnect(None, Some(wallet.url)).await.unwrap();
        let exchange_client = ExchangeClient::new(None, wallet.wallet.clone(), Some(wallet.url), None, None).await.unwrap();

        //fetch user fees %
        let fees = wallet.get_user_fees().await;
        let margin = wallet.get_user_margin().await.unwrap_or(0.0);
        let meta = get_asset(&info_client, asset.as_str().trim()).await;
        
        if meta.is_none(){
            return Err(format!("Failed to fetch Metadata for the {}", asset));
        } 
        
        info!("\n MARGIN: {}", margin); 
        //setup channels
        let (market_tx, mut market_rv) = channel::<MarketCommand>(4);
        let (exec_tx, mut rv_exec) = bounded::<TradeCommand>(0);
        let (engine_tx, mut engine_rv) = unbounded_channel::<EngineCommand>();

        Ok((Market{ 
            exchange_client,
            info_client, 
            margin,
            trade_history: Vec::with_capacity(MAX_HISTORY),
            pnl: 0_f32,
            trade_params : trade_params.clone(),
            asset: meta.unwrap(), 
            signal_engine: SignalEngine::new(indicators_config, trade_params,engine_rv,exec_tx.clone(), margin).await,
            executor: Executor::new(wallet.wallet, asset, fees,rv_exec ,market_tx.clone()).await,
            market_rv, 
            engine_tx,
            exec_tx,
            url: wallet.url,
        }, market_tx,
        ))
    }
    
    async fn init(&mut self) -> Result<(), String>{
        
        //check if lev > max_lev
        let lev = self.trade_params.lev.min(self.asset.max_leverage);
        let upd = self.trade_params.update_lev(lev ,&self.exchange_client, self.asset.name.as_str()).await;
        if let Some(lev) = upd{
            let engine_tx = self.engine_tx.clone();
            let _ = engine_tx.send(EngineCommand::UpdateExecParams(ExecParam::Lev(lev)));
        };
        self.load_engine(300).await?;
        println!("\nMarket initialized for {} {:?}\n", self.asset.name, self.trade_params);
        Ok(())
    }



    pub async fn change_time_frame(&mut self, tf: TimeFrame) -> Result<(), String>{
        if tf != self.trade_params.time_frame{
            self.trade_params.time_frame = tf;
            self.signal_engine.reset();

            self.load_engine(3000).await?;
        }
        Ok(())
    }

    pub fn change_strategy(&mut self, strategy: Strategy){

        self.trade_params.strategy = strategy.clone();
        
    }
        
    async fn load_engine(&mut self, candle_count: u64) -> Result<(), String>{

        let price_data = load_candles(&self.info_client, self.asset.name.as_str(), self.trade_params.time_frame, candle_count)
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
        let (shutdown_tx, mut receiver) = subscribe_candles(self.url,self.asset.name.as_str(), self.trade_params.time_frame.as_str()).await;



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
                        let lev = lev.min(self.asset.max_leverage);
                        let upd = self.trade_params.update_lev(lev ,&self.exchange_client, self.asset.name.as_str()).await;
                        if let Some(lev) = upd{
                            let _ = engine_update_tx.send(EngineCommand::UpdateExecParams(ExecParam::Lev(lev)));
                    };
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
                        self.margin += trade_info.pnl;
                        self.trade_history.push(trade_info);
                        let _ = engine_update_tx.send(EngineCommand::UpdateExecParams(ExecParam::Margin(self.margin)));
                    },
                    MarketCommand::UpdateTimeFrame(tf)=>{
                        
                        let price_data = load_candles(&self.info_client,
                                                    self.asset.name.as_str(),
                                                    tf,
                                                    3000,
                                                        ).await; 
                        if let Ok(price_data) = price_data{
                            self.trade_params.time_frame = tf;
                            let _ = engine_update_tx.send(EngineCommand::Reload{price_data, tf});
                        };
                    },
                    MarketCommand::Pause =>{

                       self.exec_tx.send_async(TradeCommand::Pause).await;  
                    },

                    MarketCommand::Close=>{
                    info!("\nClosing {} Market...\n", self.asset.name);
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
                                        self.margin += trade_info.pnl;
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












