use crate::helper::address;
use alloy::signers::local::PrivateKeySigner;
use hyperliquid_rust_sdk::{BaseUrl, Error, InfoClient, UserFillsResponse};

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
        let add_fee: f64 = user_fees.user_add_rate.parse().unwrap();
        let cross_fee: f64 = user_fees.user_cross_rate.parse().unwrap();

        Ok((add_fee, cross_fee))
    }

    pub async fn user_fills(&self) -> Result<Vec<UserFillsResponse>, Error> {
        let user = address(&self.pubkey);

        return self.info_client.user_fills(user).await;
    }

    pub async fn get_user_margin(&self) -> Result<f64, Error> {
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

        let upnl: f64 = info
            .asset_positions
            .into_iter()
            .filter_map(|p| {
                let u = p.position.unrealized_pnl.parse::<f64>().ok()?;
                let f = p.position.cum_funding.since_open.parse::<f64>().ok()?;
                Some(u - f)
            })
            .sum();

        Ok(res - upnl)
    }
}
