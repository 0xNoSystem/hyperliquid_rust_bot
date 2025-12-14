use hyperliquid_rust_sdk::{AssetPosition, Error};
use rustc_hash::FxHasher;
use std::hash::BuildHasherDefault;
use std::sync::Arc;

use crate::{Wallet, roundf};
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MarginAllocation {
    Alloc(f64), //percentage of available margin
    Amount(f64),
}

struct Margin {
    total: f64,
    used: f64,
}

pub type MarginMap = HashMap<String, f64, BuildHasherDefault<FxHasher>>;

pub struct MarginBook {
    user: Arc<Wallet>,
    map: MarginMap,
    pub total_on_chain: f64,
}

impl MarginBook {
    pub fn new(user: Arc<Wallet>) -> Self {
        Self {
            user,
            map: HashMap::default(),
            total_on_chain: f64::from_bits(1),
        }
    }

    pub async fn sync(&mut self) -> Result<Vec<AssetPosition>, Error> {
        let res = self.user.get_user_margin(&mut self.map.keys()).await?;
        self.total_on_chain = res.0;
        Ok(res.1)
    }

    pub async fn update_asset(&mut self, update: AssetMargin) -> Result<f64, Error> {
        let (asset, requested_margin) = update;

        let free: f64;
        if let Some(margin) = self.map.get(&asset) {
            free = self.free() + margin;
        } else {
            return Err(Error::Custom(format!("{} market doesn't exist", &asset)));
        }

        self.sync().await?;
        if requested_margin > free {
            return Err(Error::InsufficientFreeMargin(roundf!(free, 2)));
        }
        self.map.insert(asset, requested_margin);

        Ok(requested_margin)
    }

    pub async fn allocate(&mut self, asset: String, alloc: MarginAllocation) -> Result<f64, Error> {
        self.sync().await?;
        let free = self.free();

        match alloc {
            MarginAllocation::Alloc(ptc) => {
                if ptc <= 0.0 {
                    return Err(Error::InvalidMarginAmount);
                }
                let requested_margin = free * ptc;
                if requested_margin > free {
                    log::warn!("Error::InsufficientFreeMargin({})", free);
                    return Err(Error::InsufficientFreeMargin(roundf!(free, 2)));
                }
                self.map.insert(asset, requested_margin);
                Ok(requested_margin)
            }

            MarginAllocation::Amount(amount) => {
                if amount <= 0.0 {
                    return Err(Error::InvalidMarginAmount);
                }
                if amount > free {
                    log::warn!("Error::InsufficientFreeMargin({})", free);
                    return Err(Error::InsufficientFreeMargin(roundf!(free, 2)));
                }
                self.map.insert(asset, amount);
                Ok(amount)
            }
        }
    }

    pub fn remove(&mut self, asset: &String) {
        self.map.remove(asset);
    }

    pub fn used(&self) -> f64 {
        self.map.values().copied().sum()
    }

    pub fn free(&self) -> f64 {
        self.total_on_chain - self.used()
    }

    pub fn reset(&mut self) {
        self.map.clear();
    }
}

pub type AssetMargin = (String, f64);
