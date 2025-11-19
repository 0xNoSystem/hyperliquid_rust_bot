use crate::helper::address;
use alloy::signers::local::PrivateKeySigner;
use hyperliquid_rust_sdk::{AssetPosition, BaseUrl, Error, InfoClient, UserFillsResponse};

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

        let info = self.info_client.user_state(user).await?;

        let res = info
            .margin_summary
            .account_value
            .parse::<f64>()
            .map_err(|e| {
                Error::GenericParse(format!(
                    "FATAL: failed to parse account balance to f64, {}",
                    e
                ))
            })?;
        let positions = info.asset_positions;

        let upnl: f64 = positions
            .iter()
            .filter_map(|p| {
                if !bot_assets.any(|a| a == &p.position.coin) {
                    return Some(p.position.margin_used.parse::<f64>().ok()?);
                }
                let u = p.position.unrealized_pnl.parse::<f64>().ok()?;
                let f = p.position.cum_funding.since_open.parse::<f64>().ok()?;
                Some(u - f)
            })
            .sum();
        Ok((res - upnl, positions))
    }
}
