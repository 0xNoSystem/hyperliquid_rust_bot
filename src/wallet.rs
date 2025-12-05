use crate::helper::address;
use alloy::signers::local::PrivateKeySigner;
use hyperliquid_rust_sdk::{AssetPosition, BaseUrl, Error, InfoClient, UserFillsResponse};
use std::collections::HashSet;
pub struct Wallet {
    info_client: InfoClient,
    pub wallet: PrivateKeySigner,
    pub pubkey: String,
    pub url: BaseUrl,
}

impl Wallet {
    pub async fn new(
        url: BaseUrl,
        pubkey: String,
        wallet: PrivateKeySigner,
    ) -> Result<Self, Error> {
        let info_client = InfoClient::new(None, Some(url)).await?;
        Ok(Wallet {
            info_client,
            wallet,
            pubkey,
            url,
        })
    }

    pub async fn get_user_fees(&self) -> Result<(f64, f64), Error> {
        let user = address(&self.pubkey);
        let user_fees = self.info_client.user_fees(user).await?;
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
        let user = address(&self.pubkey);

        return self.info_client.user_fills(user).await;
    }

    pub async fn get_user_margin(
        &self,
        bot_assets: &mut std::collections::hash_map::Keys<'_, String, f64>,
    ) -> Result<(f64, Vec<AssetPosition>), Error> {
        let user = address(&self.pubkey);

        let state = self.info_client.user_state(user).await?;
        let open_orders = self.info_client.frontend_open_orders(user).await?;

        let res = state
            .margin_summary
            .account_value
            .parse::<f64>()
            .map_err(|e| {
                Error::GenericParse(format!(
                    "FATAL: failed to parse account balance to f64, {}",
                    e
                ))
            })?;
        let positions = state.asset_positions;

        let ass = std::sync::Arc::new(bot_assets.cloned().collect::<HashSet<String>>());
        let open_orders_value_futures = open_orders.iter().filter_map(|o| {
            Some({
                let mut assets = ass.clone();
                async move {
                    if o.is_position_tpsl || o.reduce_only {
                        return None;
                    }
                    if !assets.contains(&o.coin) {
                        let lev = self
                            .info_client
                            .active_asset_data(user, o.coin.clone())
                            .await
                            .ok()?
                            .leverage
                            .value as f64;
                        return Some(
                            (o.sz.parse::<f64>().ok()? * o.limit_px.parse::<f64>().ok()?) / lev);
                    }
                    None
                }
            })
        });

        let results = futures::future::join_all(open_orders_value_futures).await;
        let discard_value: f64 = results.into_iter().flatten().sum();

        let upnl: f64 = positions
            .iter()
            .filter_map(|p| {
                if !ass.contains(&p.position.coin) {
                    return p.position.margin_used.parse::<f64>().ok();
                }
                let u = p.position.unrealized_pnl.parse::<f64>().ok()?;
                let f = p.position.cum_funding.since_open.parse::<f64>().ok()?;
                Some(u - f)
            })
            .sum();
        Ok((res - upnl - discard_value, positions))
    }
}
