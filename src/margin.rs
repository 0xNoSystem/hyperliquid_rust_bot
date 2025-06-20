use std::hash::{Hash, BuildHasherDefault};
use rustc_hash::FxHasher;
use hyperliquid_rust_sdk::{Error};
use std::sync::Arc;

use crate::Wallet;
use std::collections::HashMap;

#[derive(Clone, Debug, Copy)]
pub enum MarginAllocation{
    Alloc(f32),
    Amount(f32),
}

pub(crate) type MarginMap = HashMap<String, f32, BuildHasherDefault<FxHasher>>;

pub(crate) struct MarginBook{
    user: Arc<Wallet>,
    pub map: MarginMap,
    pub total_on_chain: f32,
}


impl MarginBook{

    pub fn new(user: Arc<Wallet>) -> Self{
        
        Self{
            user,
            map: HashMap::default(),
            total_on_chain: 0.0,
        }
    }
    pub async fn sync(&mut self) -> Result<(), Error>{
        self.total_on_chain = self.user.get_user_margin().await?;
        Ok(())
    }


    pub fn update_asset(&mut self, update: AssetMargin) -> Result<f32, Error>{
        let (asset, requested_margin) = update;
        let free = self.free();
         
        if requested_margin > free{
            return Err(Error::InsufficientFreeMargin(free));
        }
        self.map.insert(asset.to_string(), requested_margin);

        Ok(requested_margin)
    }

    pub async fn allocate(&mut self, asset: String, alloc: MarginAllocation) -> Result<(), Error>{
        self.sync().await?;
        let free = self.free();

        match alloc{
            MarginAllocation::Alloc(ptc)=>{
                let requested_margin = self.total_on_chain * ptc;
                if requested_margin > free{
                    return Err(Error::InsufficientFreeMargin(free));
                }
                self.map.insert(asset, requested_margin);
            },

            MarginAllocation::Amount(amount)=>{
                if amount > free{
                    return Err(Error::InsufficientFreeMargin(free));
                }
                self.map.insert(asset, amount);
            },
        }

        Ok(())
    } 

    pub fn remove(&mut self, asset: String) {
        self.map.remove(&asset);
    }

    pub fn used(&self) -> f32{
        self.map.values().copied().sum()
    }

    pub fn free(&self) -> f32{
        self.total_on_chain - self.used()
    }




}


pub type AssetMargin = (Arc<str>, f32);




