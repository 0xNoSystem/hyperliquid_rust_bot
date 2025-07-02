#![allow(unused_imports)]
#![allow(unused_mut)]
#![allow(unused_variables)]

use log::info;
use std::str::FromStr;
use std::sync::Arc;
use hyperliquid_rust_sdk::Error;
use hyperliquid_rust_bot::{
    Bot,
    BotEvent,
    MarginAllocation,
    AssetMargin,
    BotToMarket,
    MarketCommand,
    IndexId, Entry, EditType, IndicatorKind,
    MARKETS,
    TradeParams,TimeFrame, AddMarketInfo, UpdateFrontend,

    LocalWallet, Wallet, BaseUrl,
};
use hyperliquid_rust_bot::strategy::{Strategy, CustomStrategy, Risk, Style, Stance};

use tokio::{
    sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    time::{sleep, Duration},
};



#[tokio::main]
async fn main() -> Result<(), Error>{
    env_logger::init();
    dotenv::dotenv().ok();

    let wallet = load_wallet(BaseUrl::Mainnet).await?;


    let strategy = Strategy::Custom(load_strategy("./config.toml"));
    let trade_params = TradeParams{
        strategy,
        lev: 10,
        trade_time: 600,
        time_frame: TimeFrame::from_str("5m").unwrap_or(TimeFrame::Min1),
    };

    let change = (IndicatorKind::Rsi(12), TimeFrame::Min5); 
    let change2 = (IndicatorKind::EmaCross{short: 20, long: 200}, TimeFrame::Day1);

    let edit1 = Entry{id: (IndicatorKind::Rsi(12), TimeFrame::Min5) ,edit: EditType::Remove};

    let market_info = AddMarketInfo{
        asset: "BTC".to_string(),
        margin_alloc: MarginAllocation::Amount(50.5),
        trade_params,
        config: Some(Vec::from([change, change2])), //indicators config
    }; 

    println!("{:?}", market_info);

       
    let (mut bot, event_tx) = Bot::new(wallet).await?;
    let (app_tx, mut app_rv) = unbounded_channel::<UpdateFrontend>();
    
    tokio::spawn(async move{
        bot.start(app_tx).await;
    });     

    let _ = event_tx.send(BotEvent::AddMarket(market_info.clone()));
    let _ = event_tx.send(BotEvent::AddMarket(market_info));
    let _ = event_tx.send(BotEvent::ManualUpdateMargin(("BTC".into(),100.0)));
    let _ = event_tx.send(BotEvent::MarketComm(BotToMarket{
        asset: "BTC".to_string(),
        cmd: MarketCommand::UpdateLeverage(33),
    }));

    while let Some(update) = app_rv.recv().await{
            let json_update = serde_json::to_string(&update).unwrap();
            println!("FRONT END RECEIVED SERIALIZED DATA: {}", json_update);
    }


    Ok(())
}


fn load_strategy(path: &str) -> CustomStrategy {
    let content = std::fs::read_to_string(path).expect("failed to read file");
    toml::from_str(&content).expect("failed to parse toml")
}

async fn load_wallet(url: BaseUrl) -> Result<Wallet, Error>{
    let wallet = std::env::var("PRIVATE_KEY").expect("Error fetching PRIVATE_KEY")
        .parse();

    if let Err(ref e) = wallet{
        return Err(Error::Custom(format!("Failed to load wallet: {}", e))); 
    }
    let pubkey: String = std::env::var("WALLET").expect("Error fetching WALLET address");
    Ok(Wallet::new(url , pubkey, wallet.unwrap()).await?)
}


/*
*
*
* pub enum BotEvent{
    AddMarket(AddMarketInfo),
    ToggleMarket(String),
    RemoveMarket(String),
    MarketComm(BotToMarket),
    ManualUpdateMargin(AssetMargin),
    ResumeAll,
    PauseAll,
    CloseAll,
}

#[derive(Clone, Debug)]
pub struct BotToMarket{
    pub asset: String,
    pub cmd: MarketCommand,
}


#[derive(Clone, Debug)]
pub enum UpdateFrontend{
    UpdatePrice(AssetPrice),
    NewTradeInfo(TradeInfo),
    UpdateTotalMargin(f64),
    UpdateMarketMargin(AssetMargin),
    UserError(Error),
    UpdateMarketInfo .....
}


#[derive(Clone, Debug)]
pub struct AddMarketInfo {
    pub asset: String,
    pub margin_alloc: MarginAllocation,
    pub trade_params: TradeParams,
    pub config: Option<Vec<IndexId>>,
}

#[derive(Debug, Clone)]
pub enum MarketCommand{
    UpdateLeverage(u32),
    UpdateStrategy(Strategy),
    EditIndicators(Vec<Entry>),
    UpdateTimeFrame(TimeFrame),
    ReceiveTrade(TradeInfo),
    ReceiveLiquidation(LiquidationFillInfo),
    UpdateMargin(f64),
    Toggle,
    Resume,
    Pause,
    Close,
}

????
MarketInfo{
    Lev,
    Strategy,
    Indicators,
    TimeFrame,
    Trades, 
    Pnl,
    Margin, 
    PAUSED ?
}
*/
