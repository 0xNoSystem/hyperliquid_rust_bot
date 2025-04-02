#![allow(unused_imports)]
#![allow(unused_mut)]
#![allow(unused_variables)]

use hyperliquid_rust_bot::helper::load_candles;
use kwant::indicators::{Rsi, Indicator, Price, Ema, Adx, Atr, EmaCross};
use hyperliquid_rust_sdk::{InfoClient, BaseUrl};
use tokio::time::{Duration, sleep};

#[tokio::main]
async fn main(){
    let mut info_client = InfoClient::new(None, Some(BaseUrl::Mainnet)).await.unwrap();

    for i in 0..5{
        
        let prices = load_candles(&info_client, "SOL", "1m", 300 ).await;

        //let mut ema = Ema::new(9);
        let mut rsi = Rsi::new(14, 14, Some(10));
        let mut adx = Adx::new(14, 14);
        let mut atr = Atr::new(14);
        let mut ema_cross = EmaCross::new(9, 30);

        rsi.load(&prices);
        ema_cross.load(&prices);
        adx.load(&prices);
        atr.load(&prices);

        if let Some(rsi_value) = rsi.get_last(){
            println!("RSI: {}", rsi_value);
        }

        if let Some(stoch) = rsi.get_stoch_rsi(){
            println!("🔵STOCH-K: {}", stoch);
        }

        if let Some(sma) = rsi.get_sma_rsi(){
            println!("SMA: {}", sma);
        }
        if let Some(stoch_signal) = rsi.get_stoch_signal(){
            println!("🟠STOCH SIGNAL-D: {}\n\n", stoch_signal);
        }

        if let Some(ema_slope) = ema_cross.long.get_slope(){
            println!("EMA SLOPE: {}", ema_slope);
        }
        if let Some(sma_value) = ema_cross.short.get_sma(){
            println!("SMA: {}", sma_value);
        }

        if let Some(adx_value) = adx.get_last(){
            println!("🔴ADX : {}", adx_value);
        }

        if let Some(atr_value) = atr.get_last(){
            println!("ATR: {}", atr_value);
        }

        if let Some(ema) = ema_cross.short.get_last(){
            println!("SHORT EMA: {}", ema);
        }
            
        if let Some(trend) = ema_cross.get_trend(){
            println!("EMA CROSS UPTREND: {}", trend );
        }

        //println!("{:?}", ema_cross.update(Price{high: 130.0, low: 127.0, open: 128.2, close: 110.0}, true));
        sleep(Duration::from_secs(5)).await;
    }

}