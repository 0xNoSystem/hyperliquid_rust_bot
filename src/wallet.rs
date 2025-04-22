use ethers::signers::LocalWallet;
use crate::helper::{address};
use hyperliquid_rust_sdk::{InfoClient, UserFillsResponse, BaseUrl};

pub struct Wallet{
    info_client: InfoClient,
    pub wallet: LocalWallet,
    pub pubkey: String,
    pub url: BaseUrl,
}


impl Wallet{

    pub async fn new(url: BaseUrl,pubkey: String, wallet: LocalWallet) -> Self{

        let mut info_client = InfoClient::new(None, Some(url)).await.unwrap();
        Wallet{
            info_client,
            wallet,
            pubkey,
            url,
        }
    }

    pub async fn get_user_fees(&self) -> (f32, f32){
        let user = address(&self.pubkey);
        let user_fees = self.info_client.user_fees(user).await.unwrap();
        let add_fee: f32 = user_fees.user_add_rate.parse().unwrap();
        let cross_fee: f32 = user_fees.user_cross_rate.parse().unwrap();
    
        (add_fee, cross_fee)
    }

    pub async fn user_fills(&self) -> Vec<UserFillsResponse>{

        let user = address(&self.pubkey);

        return self.info_client.user_fills(user).await.unwrap();
    
    }

    pub async fn get_user_margin(&self) -> Result<f32, String> {
        let user = address(&self.pubkey);

        let info = self.info_client.user_state(user)
        .await
        .map_err(|e| format!("Error fetching user balance, {}",e))?;

        let res =  info.cross_margin_summary.account_value
        .parse::<f32>()
        .map_err(|e| format!("FATAL: failed to parse account balance to f32, {}",e))?;
        Ok(res) 
}


}

