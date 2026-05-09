use alloy::primitives::Address;

use alloy::signers::local::PrivateKeySigner;
use futures::future::join_all;
use hyperliquid_rust_sdk::{
    AssetPosition, BaseUrl, Error, FrontendOpenOrdersResponse, InfoClient, UserFillsResponse,
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct Wallet {
    dexs: Arc<RwLock<Vec<Option<String>>>>,
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
        let info_client = InfoClient::new(None, Some(url)).await?;
        let dexs = Arc::new(RwLock::new(fetch_dexs(&info_client).await?));

        let dexs_ref = Arc::clone(&dexs);
        let refresh_client = InfoClient::new(None, Some(url)).await?;
        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(tokio::time::Duration::from_secs(Self::DEXS_REFRESH_SECS));
            interval.tick().await;
            loop {
                interval.tick().await;
                match fetch_dexs(&refresh_client).await {
                    Ok(updated) => *dexs_ref.write().await = updated,
                    Err(e) => eprintln!("[wallet] dex refresh failed: {e}"),
                }
            }
        });

        Ok(Wallet {
            dexs,
            info_client,
            wallet,
            pubkey,
            url,
        })
    }

    pub async fn get_user_fees(&self) -> Result<(f64, f64), Error> {
        let user_fees = self.info_client.user_fees(self.pubkey).await?;
        let add_fee = user_fees.user_add_rate.parse::<f64>().map_err(|_| {
            Error::GenericParse(format!(
                "Failed to parse user_add_rate: {}",
                user_fees.user_add_rate
            ))
        })?;

        let cross_fee = user_fees.user_cross_rate.parse::<f64>().map_err(|_| {
            Error::GenericParse(format!(
                "Failed to parse user_cross_rate: {}",
                user_fees.user_cross_rate
            ))
        })?;

        Ok((add_fee, cross_fee))
    }

    pub async fn user_fills(&self) -> Result<Vec<UserFillsResponse>, Error> {
        self.info_client.user_fills(self.pubkey).await
    }

    async fn get_all_positions(&self) -> Result<(Vec<AssetPosition>, f64), Error> {
        let dexs = self.dexs.read().await.clone();
        let futures = dexs
            .iter()
            .map(|d| self.info_client.user_state(self.pubkey, d.clone()));

        let is_unified = self
            .info_client
            .get_user_abstraction(self.pubkey)
            .await?
            .is_unified();

        let mut account_value = if is_unified {
            self.info_client
                .user_token_balances(self.pubkey)
                .await?
                .balances[0]
                .total
                .parse::<f64>()
                .map_err(|e| {
                    Error::GenericParse(format!(
                        "FATAL: failed to parse account balance to f64, {}",
                        e
                    ))
                })?
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
                    match state.margin_summary.account_value.parse::<f64>() {
                        Ok(v) => account_value += v,
                        Err(e) => {
                            parse_error = Some(Error::GenericParse(format!(
                                "FATAL: failed to parse account balance to f64, {}",
                                e
                            )))
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
            self.info_client
                .frontend_open_orders(self.pubkey, d.clone())
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

        let ((positions, account_value), open_orders) =
            tokio::try_join!(self.get_all_positions(), self.get_all_orders())?;

        let unknown_coins: HashSet<&str> = open_orders
            .iter()
            .filter(|o| !o.is_position_tpsl && !o.reduce_only && !bot_assets.contains(&o.coin))
            .map(|o| o.coin.as_str())
            .collect();

        let leverage_map: HashMap<&str, f64> =
            join_all(unknown_coins.iter().map(|&coin| async move {
                let lev = self
                    .info_client
                    .active_asset_data(self.pubkey, coin.to_string())
                    .await
                    .ok()?
                    .leverage
                    .value as f64;
                Some((coin, lev))
            }))
            .await
            .into_iter()
            .flatten()
            .collect();

        let discard_value: f64 = open_orders
            .iter()
            .filter(|o| !o.is_position_tpsl && !o.reduce_only)
            .filter_map(|o| {
                let lev = leverage_map.get(o.coin.as_str())?;
                Some((o.sz.parse::<f64>().ok()? * o.limit_px.parse::<f64>().ok()?) / lev)
            })
            .sum();

        let upnl: f64 = positions
            .iter()
            .filter_map(|p| {
                if !bot_assets.contains(&p.position.coin) {
                    return p.position.margin_used.parse::<f64>().ok();
                }
                let u = p.position.unrealized_pnl.parse::<f64>().ok()?;
                let f = p.position.cum_funding.since_open.parse::<f64>().ok()?;
                Some(u - f)
            })
            .sum();

        Ok((account_value - upnl - discard_value, positions))
    }
}

async fn fetch_dexs(info_client: &InfoClient) -> Result<Vec<Option<String>>, Error> {
    Ok(info_client
        .perp_dexs()
        .await?
        .into_iter()
        .map(|d| d.map(|d| d.name))
        .collect())
}
