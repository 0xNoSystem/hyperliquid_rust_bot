use log::info;
use std::collections::HashMap;
use ethers::signers::LocalWallet;
use hyperliquid_rust_sdk::{AssetMeta,BaseUrl, ExchangeClient,Error, InfoClient, Message};
use crate::{Market, MarketCommand, MarketUpdate,AssetPrice, MARKETS, TradeParams,TradeInfo, TradeFillInfo, Wallet, IndexId, Entry};
use crate::helper::{get_asset, subscribe_candles};


use tokio::{
    sync::mpsc::{channel, Sender, Receiver, UnboundedSender, UnboundedReceiver, unbounded_channel},
};


pub struct Bot{
    info_client: InfoClient,
    wallet: Wallet,
    markets: HashMap<String, Sender<MarketCommand>>,
    candle_subs: HashMap<String, u32>,
    margin: f32,
    fees: (f32, f32),
    bot_tx: UnboundedSender<BotEvent>,
    bot_rv: UnboundedReceiver<BotEvent>,
    update_rv: UnboundedReceiver<MarketUpdate>,
    update_tx: UnboundedSender<MarketUpdate>,
}





impl Bot{

    pub async fn new(wallet: Wallet) -> Result<(Self, UnboundedSender<BotEvent>), Error>{

        let mut info_client = InfoClient::with_reconnect(None, Some(wallet.url)).await?;
        let margin = wallet.get_user_margin().await?;
        let fees = wallet.get_user_fees().await?;

        let (bot_tx, mut bot_rv) = unbounded_channel::<BotEvent>();
        let (update_tx, mut update_rv) = unbounded_channel::<MarketUpdate>();

        Ok((Self{
            info_client, 
            wallet,
            markets: HashMap::new(),
            candle_subs: HashMap::new(),
            margin,
            fees,
            bot_tx: bot_tx.clone(),
            bot_rv,
            update_rv,
            update_tx,
        }, bot_tx))
    }



    pub async fn add_market(&mut self, info: AddMarketInfo) -> Result<(), Error>{
       
        let AddMarketInfo {
            asset,
            margin_alloc,
            trade_params,
            config,
                } = info;

        let asset_str = asset.trim();
        let margin_alloc = margin_alloc.min(0.99);
        if !MARKETS.contains(&asset_str){
            return Err(Error::AssetNotFound);
        }
        
        let meta = get_asset(&self.info_client, asset_str).await?;
        let margin = self.wallet.get_user_margin().await?;
        let (sub_id, mut receiver) = subscribe_candles(&mut self.info_client,
                                                        asset_str,
                                                        trade_params.time_frame.as_str())
                                                        .await?;

        self.candle_subs.insert(asset.clone(), sub_id);
        
        let (market, market_tx) = Market::new(
            self.wallet.wallet.clone(),
            self.wallet.url,
            self.update_tx.clone(),
            receiver,
            meta,     
            self.margin * margin_alloc,
            self.fees,
            trade_params,
            config,
        ).await?;

        self.markets.insert(asset.clone(), market_tx);
        tokio::spawn(async move {
            if let Err(e) = market.start().await {
                eprintln!("Market {} exited with error: {:?}", asset, e);
            }
        });         

        Ok(())
}


    pub async fn remove_market(&mut self, asset: &String) -> Result<(), Error>{
      
        let id = self.candle_subs.remove(asset);
        if let Some(sub_id) = id{
            self.info_client.unsubscribe(sub_id).await?;
            info!("Removed {} market successfully", asset);
        }else{
            info!("Couldn't remove {} market, it doesn't exist", asset);
            return Ok(());
        }

        if let Some(tx) = self.markets.remove(asset){
            let tx = tx.clone();
            let cmd = MarketCommand::Close;
            tokio::spawn(async move {
                if let Err(e) = tx.send(cmd).await{
                   log::warn!("Failed to send Pause command: {:?}", e); 
                }
            });

        }else{
            info!("Failed: Pause {} market, it doesn't exist", asset);
        }


        Ok(())
    }

    pub async fn pause_or_resume_market(&self, asset: &String){
        
        if let Some(tx) = self.markets.get(asset){
            let tx = tx.clone();
            let cmd = MarketCommand::Pause;
            tokio::spawn(async move{
                if let Err(e) =  tx.send(cmd).await{
                    log::warn!("Failed to send Pause command: {:?}", e);
                }
            });

        }else{
            info!("Failed: Pause {} market, it doesn't exist", asset);
        }
    }

    pub async fn send_cmd(&self, asset: &String, cmd: MarketCommand){
        
        if let Some(tx) = self.markets.get(asset){
            let tx = tx.clone();
            tokio::spawn(async move{
                if let Err(e) =  tx.send(cmd).await{
                    log::warn!("Failed to send Market command: {:?}", e);
                }
            });
        }
}
    
 
    pub async fn start(mut self, app_tx: UnboundedSender<UpdateFrontend>){
        use BotEvent::*; 
        use MarketUpdate::*;
        use UpdateFrontend::*;

        loop{
            tokio::select!(

                Some(event) = self.bot_rv.recv() => {
            
                    match event{
                        AddMarket(add_market_info) => {self.add_market(add_market_info).await;},
                        ToggleMarket(asset) => {self.pause_or_resume_market(&asset).await;},
                        RemoveMarket(asset) => {self.remove_market(&asset).await;},
                        MarketComm(command) => {self.send_cmd(&command.asset, command.cmd).await;},
                    }
            },

                Some(market_update) = self.update_rv.recv() => {
                    match market_update{
                        PriceUpdate(asset_price) => {app_tx.send(UpdatePrice(asset_price));},
                        TradeUpdate(trade_info) => {app_tx.send(NewTradeInfo(trade_info));},
                }
            }
        )}

    }   

}




#[derive(Clone, Debug)]
pub enum BotEvent{
    AddMarket(AddMarketInfo),
    ToggleMarket(String),
    RemoveMarket(String),
    MarketComm(BotToMarket),
}



#[derive(Clone, Debug)]
pub struct BotToMarket{
    asset: String,
    cmd: MarketCommand,
}


#[derive(Clone, Debug)]
pub enum UpdateFrontend{
    UpdatePrice(AssetPrice),
    NewTradeInfo(TradeInfo),
}


#[derive(Clone, Debug)]
pub struct AddMarketInfo {
    pub asset: String,
    pub margin_alloc: f32,
    pub trade_params: TradeParams,
    pub config: Option<Vec<IndexId>>,
}

