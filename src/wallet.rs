use alloy::primitives::Address;

use alloy::signers::local::PrivateKeySigner;
use futures::future::join_all;
use hyperliquid_rust_sdk::{
    AssetPosition, BaseUrl, Error, FrontendOpenOrdersResponse, InfoClient, UserFillsResponse,
};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time::timeout;

static MAINNET_DEXS: OnceLock<Arc<RwLock<Vec<Option<String>>>>> = OnceLock::new();
static TESTNET_DEXS: OnceLock<Arc<RwLock<Vec<Option<String>>>>> = OnceLock::new();
static LOCALHOST_DEXS: OnceLock<Arc<RwLock<Vec<Option<String>>>>> = OnceLock::new();
static MAINNET_DEXS_REFRESH_STARTED: AtomicBool = AtomicBool::new(false);
static TESTNET_DEXS_REFRESH_STARTED: AtomicBool = AtomicBool::new(false);
static LOCALHOST_DEXS_REFRESH_STARTED: AtomicBool = AtomicBool::new(false);
const HL_INFO_TIMEOUT_SECS: u64 = 15;
const LEVERAGE_CACHE_TTL_SECS: u64 = 30;

pub struct Wallet {
    dexs: Arc<RwLock<Vec<Option<String>>>>,
    leverage_cache: RwLock<HashMap<String, (f64, Instant)>>,
    info_client: InfoClient,
    pub wallet: PrivateKeySigner,
    pub pubkey: Address,
    pub url: BaseUrl,
}

impl Wallet {
    const DEXS_REFRESH_SECS: u64 = 12 * 3600;
    pub async fn new(
        url: BaseUrl,
        pubkey: Address,
        wallet: PrivateKeySigner,
    ) -> Result<Self, Error> {
        let info_client =
            hl_info_timeout("InfoClient::new", InfoClient::new(None, Some(url))).await?;
        let dexs = shared_dexs(url, &info_client).await?;
        start_dex_refresh(url, Arc::clone(&dexs));

        Ok(Wallet {
            dexs,
            leverage_cache: RwLock::new(HashMap::new()),
            info_client,
            wallet,
            pubkey,
            url,
        })
    }

    pub async fn get_user_fees(&self) -> Result<(f64, f64), Error> {
        let user_fees =
            hl_info_timeout("user_fees", self.info_client.user_fees(self.pubkey)).await?;
        let add_fee = parse_finite_f64("user_add_rate", &user_fees.user_add_rate)?;

        let cross_fee = parse_finite_f64("user_cross_rate", &user_fees.user_cross_rate)?;

        Ok((add_fee, cross_fee))
    }

    pub async fn user_fills(&self) -> Result<Vec<UserFillsResponse>, Error> {
        hl_info_timeout("user_fills", self.info_client.user_fills(self.pubkey)).await
    }

    async fn get_all_positions(&self) -> Result<(Vec<AssetPosition>, f64), Error> {
        let dexs = self.dexs.read().await.clone();
        let futures = dexs.iter().map(|d| {
            hl_info_timeout(
                "user_state",
                self.info_client.user_state(self.pubkey, d.clone()),
            )
        });

        let is_unified = hl_info_timeout(
            "get_user_abstraction",
            self.info_client.get_user_abstraction(self.pubkey),
        )
        .await?
        .is_unified();

        let mut account_value = if is_unified {
            let balances = hl_info_timeout(
                "user_token_balances",
                self.info_client.user_token_balances(self.pubkey),
            )
            .await?;
            let balance = balances.balances.first().ok_or_else(|| {
                Error::Custom("user token balances response was empty".to_string())
            })?;
            parse_finite_f64("account balance", &balance.total)?
        } else {
            0f64
        };
        let mut parse_error: Option<Error> = None;

        let r = join_all(futures)
            .await
            .into_iter()
            .collect::<Result<Vec<_>, Error>>()?
            .into_iter()
            .flat_map(|state| {
                if !is_unified {
                    match parse_finite_f64("account balance", &state.margin_summary.account_value) {
                        Ok(v) => account_value += v,
                        Err(e) => {
                            parse_error = Some(e);
                        }
                    }
                }
                state.asset_positions
            })
            .collect::<Vec<AssetPosition>>();

        if let Some(e) = parse_error {
            return Err(e);
        }

        Ok((r, account_value))
    }

    async fn get_all_orders(&self) -> Result<Vec<FrontendOpenOrdersResponse>, Error> {
        let dexs = self.dexs.read().await.clone();
        let futures = dexs.iter().map(|d| {
            hl_info_timeout(
                "frontend_open_orders",
                self.info_client
                    .frontend_open_orders(self.pubkey, d.clone()),
            )
        });
        let r = join_all(futures)
            .await
            .into_iter()
            .collect::<Result<Vec<_>, Error>>()?
            .into_iter()
            .flatten()
            .collect();
        Ok(r)
    }

    pub async fn get_user_margin(
        &self,
        bot_assets: &mut std::collections::hash_map::Keys<'_, String, f64>,
    ) -> Result<(f64, Vec<AssetPosition>), Error> {
        let bot_assets: HashSet<String> = bot_assets.cloned().collect();
        self.get_user_margin_for_assets(&bot_assets).await
    }

    pub async fn get_user_margin_for_assets(
        &self,
        bot_assets: &HashSet<String>,
    ) -> Result<(f64, Vec<AssetPosition>), Error> {
        let ((positions, account_value), open_orders) =
            tokio::try_join!(self.get_all_positions(), self.get_all_orders())?;

        let unknown_coins: Vec<String> = open_orders
            .iter()
            .filter(|o| !o.is_position_tpsl && !o.reduce_only && !bot_assets.contains(&o.coin))
            .map(|o| o.coin.clone())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        let leverage_results =
            join_all(unknown_coins.iter().map(|coin| async move {
                (coin.clone(), self.active_leverage_cached(coin).await)
            }))
            .await;
        let mut leverage_map = HashMap::new();
        let mut fallback_leverage_coins = HashSet::new();
        for (coin, result) in leverage_results {
            match result {
                Ok(leverage) if leverage.is_finite() && leverage > 0.0 => {
                    leverage_map.insert(coin, leverage);
                }
                Ok(leverage) => {
                    log::warn!(
                        "[wallet] invalid leverage {leverage} for {coin}; reserving unknown open-order margin at 1x"
                    );
                    fallback_leverage_coins.insert(coin);
                }
                Err(err) => {
                    log::warn!(
                        "[wallet] failed to fetch leverage for {coin}: {err}; reserving unknown open-order margin at 1x"
                    );
                    fallback_leverage_coins.insert(coin);
                }
            }
        }

        let discard_value = unknown_open_order_margin(
            &open_orders,
            bot_assets,
            &leverage_map,
            &fallback_leverage_coins,
        )?;

        let upnl = position_margin_adjustment(&positions, bot_assets)?;

        Ok((account_value - upnl - discard_value, positions))
    }

    async fn active_leverage_cached(&self, coin: &str) -> Result<f64, Error> {
        if let Some((leverage, fetched_at)) = self.leverage_cache.read().await.get(coin).copied()
            && fetched_at.elapsed() < Duration::from_secs(LEVERAGE_CACHE_TTL_SECS)
        {
            return Ok(leverage);
        }

        let leverage = hl_info_timeout(
            "active_asset_data",
            self.info_client
                .active_asset_data(self.pubkey, coin.to_string()),
        )
        .await?
        .leverage
        .value as f64;

        self.leverage_cache
            .write()
            .await
            .insert(coin.to_string(), (leverage, Instant::now()));

        Ok(leverage)
    }
}

fn unknown_open_order_margin(
    open_orders: &[FrontendOpenOrdersResponse],
    bot_assets: &HashSet<String>,
    leverage_map: &HashMap<String, f64>,
    fallback_leverage_coins: &HashSet<String>,
) -> Result<f64, Error> {
    let mut discard_value = 0.0;

    for order in open_orders
        .iter()
        .filter(|order| !order.is_position_tpsl && !order.reduce_only)
        .filter(|order| !bot_assets.contains(&order.coin))
    {
        let leverage = leverage_map
            .get(&order.coin)
            .copied()
            .or_else(|| fallback_leverage_coins.contains(&order.coin).then_some(1.0))
            .unwrap_or(1.0);
        let size = parse_finite_f64(&format!("open order size for {}", order.coin), &order.sz)?;
        let price = parse_finite_f64(
            &format!("open order limit price for {}", order.coin),
            &order.limit_px,
        )?;

        discard_value += (size * price) / leverage;
    }

    Ok(discard_value)
}

fn position_margin_adjustment(
    positions: &[AssetPosition],
    bot_assets: &HashSet<String>,
) -> Result<f64, Error> {
    let mut upnl = 0.0;

    for position in positions {
        if !bot_assets.contains(&position.position.coin) {
            upnl += parse_finite_f64("position margin_used", &position.position.margin_used)?;
            continue;
        }

        let unrealized =
            parse_finite_f64("position unrealized_pnl", &position.position.unrealized_pnl)?;
        let funding = parse_finite_f64(
            "position funding_since_open",
            &position.position.cum_funding.since_open,
        )?;
        upnl += unrealized - funding;
    }

    Ok(upnl)
}

fn parse_finite_f64(label: &str, raw: &str) -> Result<f64, Error> {
    let value = raw
        .parse::<f64>()
        .map_err(|err| Error::GenericParse(format!("failed to parse {label}: {err}")))?;

    if !value.is_finite() {
        return Err(Error::GenericParse(format!("{label} was not finite")));
    }

    Ok(value)
}

async fn fetch_dexs(info_client: &InfoClient) -> Result<Vec<Option<String>>, Error> {
    Ok(hl_info_timeout("perp_dexs", info_client.perp_dexs())
        .await?
        .into_iter()
        .map(|d| d.map(|d| d.name))
        .collect())
}

async fn shared_dexs(
    url: BaseUrl,
    info_client: &InfoClient,
) -> Result<Arc<RwLock<Vec<Option<String>>>>, Error> {
    let cache = dex_cache(url);
    if let Some(dexs) = cache.get() {
        return Ok(Arc::clone(dexs));
    }

    let fetched = Arc::new(RwLock::new(fetch_dexs(info_client).await?));
    let _ = cache.set(Arc::clone(&fetched));
    Ok(cache.get().map(Arc::clone).unwrap_or(fetched))
}

fn start_dex_refresh(url: BaseUrl, dexs: Arc<RwLock<Vec<Option<String>>>>) {
    let refresh_started = dex_refresh_started(url);
    if refresh_started.swap(true, Ordering::AcqRel) {
        return;
    }

    tokio::spawn(async move {
        let refresh_client = match hl_info_timeout(
            "InfoClient::new refresh",
            InfoClient::new(None, Some(url)),
        )
        .await
        {
            Ok(client) => client,
            Err(e) => {
                log::warn!("[wallet] failed to start dex refresh client: {e}");
                dex_refresh_started(url).store(false, Ordering::Release);
                return;
            }
        };
        let mut interval =
            tokio::time::interval(tokio::time::Duration::from_secs(Wallet::DEXS_REFRESH_SECS));
        interval.tick().await;
        loop {
            interval.tick().await;
            match fetch_dexs(&refresh_client).await {
                Ok(updated) => *dexs.write().await = updated,
                Err(e) => log::warn!("[wallet] dex refresh failed: {e}"),
            }
        }
    });
}

fn dex_cache(url: BaseUrl) -> &'static OnceLock<Arc<RwLock<Vec<Option<String>>>>> {
    match url {
        BaseUrl::Mainnet => &MAINNET_DEXS,
        BaseUrl::Testnet => &TESTNET_DEXS,
        BaseUrl::Localhost => &LOCALHOST_DEXS,
    }
}

fn dex_refresh_started(url: BaseUrl) -> &'static AtomicBool {
    match url {
        BaseUrl::Mainnet => &MAINNET_DEXS_REFRESH_STARTED,
        BaseUrl::Testnet => &TESTNET_DEXS_REFRESH_STARTED,
        BaseUrl::Localhost => &LOCALHOST_DEXS_REFRESH_STARTED,
    }
}

async fn hl_info_timeout<T, F>(label: &'static str, fut: F) -> Result<T, Error>
where
    F: Future<Output = Result<T, Error>>,
{
    hl_info_timeout_for(Duration::from_secs(HL_INFO_TIMEOUT_SECS), label, fut).await
}

async fn hl_info_timeout_for<T, F>(
    duration: Duration,
    label: &'static str,
    fut: F,
) -> Result<T, Error>
where
    F: Future<Output = Result<T, Error>>,
{
    timeout(duration, fut)
        .await
        .map_err(|_| Error::Custom(format!("Hyperliquid info request timed out: {label}")))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_order(coin: &str, sz: &str, limit_px: &str) -> FrontendOpenOrdersResponse {
        FrontendOpenOrdersResponse {
            coin: coin.to_string(),
            is_position_tpsl: false,
            is_trigger: false,
            limit_px: limit_px.to_string(),
            oid: 1,
            order_type: "Limit".to_string(),
            orig_sz: sz.to_string(),
            reduce_only: false,
            side: "B".to_string(),
            sz: sz.to_string(),
            timestamp: 0,
            trigger_condition: String::new(),
            trigger_px: "0".to_string(),
        }
    }

    #[tokio::test]
    async fn hl_info_timeout_reports_stalled_future() {
        let result = hl_info_timeout_for(
            Duration::from_millis(1),
            "test",
            std::future::pending::<Result<(), Error>>(),
        )
        .await;

        assert!(matches!(result, Err(Error::Custom(message)) if message.contains("timed out")));
    }

    #[test]
    fn unknown_open_order_margin_uses_conservative_fallback_leverage() {
        let orders = vec![open_order("ETH", "2.0", "100.0")];
        let bot_assets = HashSet::new();
        let leverage_map = HashMap::new();
        let fallback = HashSet::from(["ETH".to_string()]);

        let margin = unknown_open_order_margin(&orders, &bot_assets, &leverage_map, &fallback)
            .expect("fallback margin should parse");

        assert_eq!(margin, 200.0);
    }

    #[test]
    fn finite_float_parser_rejects_nan_and_infinity() {
        assert_eq!(
            parse_finite_f64("value", "1.25").expect("finite should parse"),
            1.25
        );
        assert!(parse_finite_f64("value", "NaN").is_err());
        assert!(parse_finite_f64("value", "inf").is_err());
    }

    #[test]
    fn unknown_open_order_margin_rejects_non_finite_order_values() {
        let orders = vec![open_order("ETH", "NaN", "100.0")];
        let bot_assets = HashSet::new();
        let leverage_map = HashMap::from([("ETH".to_string(), 2.0)]);
        let fallback = HashSet::new();

        assert!(unknown_open_order_margin(&orders, &bot_assets, &leverage_map, &fallback).is_err());
    }
}
