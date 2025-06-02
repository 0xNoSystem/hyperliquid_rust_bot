use log::info;
use std::collections::HashMap;
use ethers::signers::LocalWallet;
use hyperliquid_rust_sdk::{AssetMeta,BaseUrl, ExchangeClient,Error, InfoClient, Message};
use crate::{Market, MarketCommand, MarketUpdate, MARKETS, TradeParams, Wallet, IndexId, Entry};
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
    bot_tx: UnboundedSender<MarketUpdate>,
    bot_rv: UnboundedReceiver<MarketUpdate>,
}





impl Bot{

    pub async fn new(wallet: LocalWallet, url: BaseUrl) -> Result<Self, Error>{

        let wallet = Wallet::new(url, wallet).await?;
        let mut info_client = InfoClient::with_reconnect(None, Some(url)).await?;
        let margin = wallet.get_user_margin().await?;
        let fees = wallet.get_user_fees().await?;

        let (bot_tx, mut bot_rv) = unbounded_channel::<MarketUpdate>();

        Ok(Self{
            info_client, 
            wallet,
            markets: HashMap::new(),
            candle_subs: HashMap::new(),
            margin,
            fees,
            bot_tx,
            bot_rv,
        })
    }



    pub async fn add_market(&mut self, asset: String, margin_alloc: f32, trade_params: TradeParams, config: Option<Vec<IndexId>>) -> Result<(), Error>{
        
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
            self.bot_tx.clone(),
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







}
