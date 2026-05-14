use crate::{Price, TimeFrame, metrics};
use alloy::signers::local::PrivateKeySigner;
use hyperliquid_rust_sdk::{
    AssetMeta, BaseUrl, CandleData, Error, ExchangeClient, InfoClient, Message, Subscription,
};
use log::{info, warn};
use std::future::Future;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc::{
    Receiver, UnboundedSender, channel, error::TrySendError, unbounded_channel,
};
use tokio::time::timeout;

use alloy::primitives::Address;

const SDK_CANDLE_BRIDGE_CHANNEL_SIZE: usize = 1024;
const EXCHANGE_CLIENT_CONNECT_TIMEOUT_SECS: u64 = 10;
const INFO_CLIENT_CONNECT_TIMEOUT_SECS: u64 = 10;
const INFO_CLIENT_CALL_TIMEOUT_SECS: u64 = 15;
const INFO_CLIENT_WS_SUBSCRIBE_TIMEOUT_SECS: u64 = 10;
const INFO_CLIENT_WS_UNSUBSCRIBE_TIMEOUT_SECS: u64 = 10;
const INFO_CLIENT_WS_SHUTDOWN_TIMEOUT_SECS: u64 = 10;

pub async fn info_client_with_timeout(label: &str, base_url: BaseUrl) -> Result<InfoClient, Error> {
    timeout(
        Duration::from_secs(INFO_CLIENT_CONNECT_TIMEOUT_SECS),
        InfoClient::new(None, Some(base_url)),
    )
    .await
    .map_err(|_| Error::Custom(format!("{label} info client connect timed out")))?
}

pub async fn info_client_with_reconnect_timeout(
    label: &str,
    base_url: BaseUrl,
) -> Result<InfoClient, Error> {
    timeout(
        Duration::from_secs(INFO_CLIENT_CONNECT_TIMEOUT_SECS),
        InfoClient::with_reconnect(None, Some(base_url)),
    )
    .await
    .map_err(|_| Error::Custom(format!("{label} info websocket connect timed out")))?
}

pub async fn exchange_client_with_timeout(
    label: &str,
    wallet: PrivateKeySigner,
    base_url: BaseUrl,
) -> Result<ExchangeClient, Error> {
    timeout(
        Duration::from_secs(EXCHANGE_CLIENT_CONNECT_TIMEOUT_SECS),
        ExchangeClient::new(None, wallet, Some(base_url), None, None),
    )
    .await
    .map_err(|_| Error::Custom(format!("{label} exchange client connect timed out")))?
}

pub async fn info_call_timeout<T, F>(label: &str, fut: F) -> Result<T, Error>
where
    F: Future<Output = Result<T, Error>>,
{
    timeout(Duration::from_secs(INFO_CLIENT_CALL_TIMEOUT_SECS), fut)
        .await
        .map_err(|_| Error::Custom(format!("{label} info call timed out")))?
}

pub async fn info_subscribe_timeout(
    label: &str,
    info_client: &mut InfoClient,
    subscription: Subscription,
    sender: UnboundedSender<Message>,
) -> Result<u32, Error> {
    timeout(
        Duration::from_secs(INFO_CLIENT_WS_SUBSCRIBE_TIMEOUT_SECS),
        info_client.subscribe(subscription, sender),
    )
    .await
    .map_err(|_| Error::Custom(format!("{label} websocket subscribe timed out")))?
}

pub async fn info_unsubscribe_timeout(
    label: &str,
    info_client: &mut InfoClient,
    subscription_id: u32,
) -> Result<(), Error> {
    timeout(
        Duration::from_secs(INFO_CLIENT_WS_UNSUBSCRIBE_TIMEOUT_SECS),
        info_client.unsubscribe(subscription_id),
    )
    .await
    .map_err(|_| Error::Custom(format!("{label} websocket unsubscribe timed out")))?
}

pub async fn info_shutdown_ws_timeout(
    label: &str,
    info_client: &mut InfoClient,
) -> Result<(), Error> {
    timeout(
        Duration::from_secs(INFO_CLIENT_WS_SHUTDOWN_TIMEOUT_SECS),
        info_client.shutdown_ws(),
    )
    .await
    .map_err(|_| Error::Custom(format!("{label} websocket shutdown timed out")))?
}

pub async fn subscribe_candles(
    info_client: &mut InfoClient,
    coin: &str,
) -> Result<(u32, Receiver<Message>), Error> {
    let (sdk_sender, mut sdk_receiver) = unbounded_channel();
    let (sender, receiver) = channel(SDK_CANDLE_BRIDGE_CHANNEL_SIZE);

    let subscription_id = info_subscribe_timeout(
        "candle",
        info_client,
        Subscription::Candle {
            coin: coin.to_string(),
            interval: "1m".to_string(),
        },
        sdk_sender,
    )
    .await?;
    info!("Subscribed to new candle data: {:?}", subscription_id);

    let coin = coin.to_string();
    tokio::spawn(async move {
        let mut queue_full = false;
        let mut dropped = 0_u64;
        while let Some(msg) = sdk_receiver.recv().await {
            match sender.try_send(msg) {
                Ok(()) => {
                    if queue_full {
                        info!(
                            "{} SDK candle bridge recovered after dropping {} messages",
                            coin, dropped
                        );
                        queue_full = false;
                        dropped = 0;
                    }
                }
                Err(TrySendError::Full(_)) => {
                    metrics::inc_sdk_candle_bridge_dropped();
                    dropped = dropped.saturating_add(1);
                    if !queue_full {
                        warn!("{} SDK candle bridge full; dropping candle messages", coin);
                        queue_full = true;
                    }
                }
                Err(TrySendError::Closed(_)) => break,
            }
        }
    });

    Ok((subscription_id, receiver))
}

pub fn get_time_now_and_candles_ago(candle_count: u64, tf: TimeFrame) -> (u64, u64) {
    let end = get_time_now();

    let interval = candle_count
        .saturating_mul(tf.to_secs())
        .saturating_mul(1_000);

    let start = end.saturating_sub(interval);

    (start, end)
}

pub async fn candles_snapshot(
    info_client: &InfoClient,
    coin: &str,
    time_frame: TimeFrame,
    start: u64,
    end: u64,
) -> Result<Vec<Price>, Error> {
    let vec = info_call_timeout(
        "candles_snapshot",
        info_client.candles_snapshot(coin.to_string(), time_frame.to_string(), start, end),
    )
    .await?;

    let mut res: Vec<Price> = Vec::with_capacity(vec.len());
    for candle in vec {
        let h = parse_finite_f64("high", &candle.high)?;
        let l = parse_finite_f64("low", &candle.low)?;
        let o = parse_finite_f64("open", &candle.open)?;
        let c = parse_finite_f64("close", &candle.close)?;
        let vlm = parse_finite_f64("vlm", &candle.vlm)?;

        res.push(Price {
            high: h,
            low: l,
            open: o,
            close: c,
            open_time: candle.time_open,
            close_time: candle.time_close,
            vlm,
        });
    }
    Ok(res)
}

pub async fn load_candles(
    info_client: &InfoClient,
    coin: &str,
    tf: TimeFrame,
    candle_count: u64,
) -> Result<Vec<Price>, Error> {
    let (start, end) = get_time_now_and_candles_ago(candle_count + 1, tf);

    let price_data = candles_snapshot(info_client, coin, tf, start, end).await?;

    Ok(price_data)
}

#[inline]
pub fn address(address: &str) -> Result<Address, Error> {
    address
        .parse()
        .map_err(|e| Error::Wallet(format!("Failed to parse user address {}", e)))
}

pub async fn get_asset(info_client: &InfoClient, token: &str) -> Result<AssetMeta, Error> {
    let assets = info_call_timeout("all_perp_metas", info_client.all_perp_metas()).await?;

    if let Some(asset) = assets.into_iter().find(|a| a.name == token) {
        Ok(asset)
    } else {
        Err(Error::AssetNotFound)
    }
}

pub async fn get_all_assets(info_client: &InfoClient) -> Result<Vec<AssetMeta>, Error> {
    info_call_timeout("all_perp_metas", info_client.all_perp_metas()).await
}

#[inline]
pub fn get_time_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

pub fn parse_candle(candle: CandleData) -> Result<Price, Error> {
    let h = parse_finite_f64("high", &candle.high)?;
    let l = parse_finite_f64("low", &candle.low)?;
    let o = parse_finite_f64("open", &candle.open)?;
    let c = parse_finite_f64("close", &candle.close)?;
    let vlm = parse_finite_f64("vlm", &candle.volume)?;

    Ok(Price {
        high: h,
        low: l,
        open: o,
        close: c,
        open_time: candle.time_open,
        close_time: candle.time_close,
        vlm,
    })
}

fn parse_finite_f64(label: &str, raw: &str) -> Result<f64, Error> {
    let value = raw
        .parse::<f64>()
        .map_err(|e| Error::GenericParse(format!("Failed to parse {label}: {e}")))?;

    if !value.is_finite() {
        return Err(Error::GenericParse(format!("{label} was not finite")));
    }

    Ok(value)
}

#[macro_export]
macro_rules! roundf {
    ($arg:expr, $dp: expr) => {
        $crate::helper::round_ndp($arg, $dp)
    };
}

pub fn round_ndp(value: f64, dp: u32) -> f64 {
    if !value.is_finite() {
        return value;
    }

    let factor = 10_f64.powi(dp.min(15) as i32);
    (value * factor).round() / factor
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Debug)]
pub struct TimeDelta(u64);

#[macro_export]
macro_rules! timedelta {
    ($tf:path, $count:expr) => {
        $crate::helper::TimeDelta::from_tf($tf, $count)
    };
}

impl TimeDelta {
    pub(crate) const fn from_tf(tf: TimeFrame, count: u64) -> TimeDelta {
        let ms = tf.to_millis().saturating_mul(count);
        TimeDelta(ms)
    }

    pub fn as_ms(&self) -> u64 {
        self.0
    }

    pub fn as_secs(&self) -> u64 {
        self.0 / 1000
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_ndp_rounds_without_string_parse() {
        assert_eq!(round_ndp(12.3456, 2), 12.35);
        assert_eq!(round_ndp(12.344, 2), 12.34);
        assert_eq!(round_ndp(-1.235, 2), -1.24);
    }

    #[test]
    fn round_ndp_preserves_non_finite_values() {
        assert!(round_ndp(f64::NAN, 2).is_nan());
        assert_eq!(round_ndp(f64::INFINITY, 2), f64::INFINITY);
        assert_eq!(round_ndp(f64::NEG_INFINITY, 2), f64::NEG_INFINITY);
    }

    #[test]
    fn parse_finite_f64_rejects_nan_and_infinity() {
        assert_eq!(parse_finite_f64("value", "1.25").unwrap(), 1.25);
        assert!(parse_finite_f64("value", "NaN").is_err());
        assert!(parse_finite_f64("value", "inf").is_err());
        assert!(parse_finite_f64("value", "-inf").is_err());
    }

    #[test]
    fn time_delta_saturates_on_overflow() {
        let delta = TimeDelta::from_tf(TimeFrame::Month, u64::MAX);
        assert_eq!(delta.as_ms(), u64::MAX);
    }
}
