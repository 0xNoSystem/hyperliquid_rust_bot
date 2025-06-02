use ethers::signers::{LocalWallet, Signer};
use crate::helper::{address};
use hyperliquid_rust_sdk::{Error,InfoClient, UserFillsResponse, BaseUrl};

pub struct Wallet{
    info_client: InfoClient,
    pub wallet: LocalWallet,
    pub url: BaseUrl,
}


impl Wallet{

    pub async fn new(url: BaseUrl, wallet: LocalWallet) -> Result<Self, Error>{

        let mut info_client = InfoClient::new(None, Some(url)).await?;
        Ok(Wallet{
            info_client,
            wallet,
            url,
        })
    }

    pub async fn get_user_fees(&self) -> Result<(f32, f32), Error>{
        let user_fees = self.info_client.user_fees(self.wallet.address()).await?;
        let add_fee: f32 = user_fees.user_add_rate.parse().unwrap();
        let cross_fee: f32 = user_fees.user_cross_rate.parse().unwrap();
    
        Ok((add_fee, cross_fee))
    }

    pub async fn user_fills(&self) -> Result<Vec<UserFillsResponse>, Error>{

        return self.info_client.user_fills(self.wallet.address()).await;

    
    }

    pub async fn get_user_margin(&self) -> Result<f32, Error> {

        let info = self.info_client.user_state(self.wallet.address())
        .await?;

        info.cross_margin_summary.account_value
        .parse::<f32>()
        .map_err(|e| Error::GenericParse(format!("FATAL: failed to parse account balance to f32, {}",e)))

}


}

