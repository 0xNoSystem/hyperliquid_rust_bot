use hyperliquid_rust_sdk::{AssetPosition, Error};
use rustc_hash::FxHasher;
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::hash::BuildHasherDefault;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Semaphore, mpsc::Sender};

use crate::{Wallet, roundf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MarginAllocation {
    Alloc(f64), //percentage of available margin
    Amount(f64),
}

pub type MarginMap = HashMap<String, f64, BuildHasherDefault<FxHasher>>;
const MAX_CONCURRENT_MARGIN_SYNCS: usize = 8;
const MAX_MARGIN_SYNC_RETRIES: usize = 3;
const MARGIN_SYNC_TIMEOUT_SECS: u64 = 15;
static MARGIN_SYNC_LIMIT: Semaphore = Semaphore::const_new(MAX_CONCURRENT_MARGIN_SYNCS);

pub struct MarginBook {
    user: Arc<Wallet>,
    map: MarginMap,
    pub total_on_chain: f64,
    last_sync: Option<Instant>,
    sync_reset_tx: Option<Sender<()>>,
    version: u64,
}

struct MarginSyncRequest {
    user: Arc<Wallet>,
    bot_assets: HashSet<String>,
    version: u64,
}

impl MarginBook {
    pub fn new(user: Arc<Wallet>, sync_reset_tx: Option<Sender<()>>) -> Self {
        Self {
            user,
            map: HashMap::default(),
            total_on_chain: f64::from_bits(1),
            last_sync: None,
            sync_reset_tx,
            version: 0,
        }
    }

    pub async fn sync_shared(book: &Arc<Mutex<Self>>) -> Result<Vec<AssetPosition>, Error> {
        for _ in 0..MAX_MARGIN_SYNC_RETRIES {
            let request = {
                let book = book.lock().await;
                book.sync_request()
            };

            let (total_on_chain, positions) = Self::fetch_sync(&request).await?;

            let mut book = book.lock().await;
            if book.version == request.version {
                book.apply_sync(total_on_chain);
                return Ok(positions);
            }
        }

        Err(Error::Custom(
            "margin book changed while syncing; retry later".to_string(),
        ))
    }

    pub async fn sync_total_if_stale_shared(
        book: &Arc<Mutex<Self>>,
        max_age: Duration,
    ) -> Result<f64, Error> {
        let fresh_total = {
            let book = book.lock().await;
            book.last_sync
                .is_some_and(|last_sync| last_sync.elapsed() < max_age)
                .then(|| book.available_total())
        };

        if let Some(total) = fresh_total {
            return Ok(total);
        }

        Self::sync_shared(book).await?;

        let book = book.lock().await;
        Ok(book.available_total())
    }

    pub async fn update_asset_shared(
        book: &Arc<Mutex<Self>>,
        update: AssetMargin,
    ) -> Result<f64, Error> {
        let (asset, requested_margin) = update;

        {
            let book = book.lock().await;
            if !book.map.contains_key(&asset) {
                return Err(Error::Custom(format!("{} market doesn't exist", &asset)));
            }
        }

        Self::sync_shared(book).await?;

        let mut book = book.lock().await;
        let Some(current_margin) = book.map.get(&asset).copied() else {
            return Err(Error::Custom(format!("{} market doesn't exist", &asset)));
        };
        let free = book.free() + current_margin;

        if requested_margin > free {
            return Err(Error::InsufficientFreeMargin(roundf!(free, 2)));
        }

        book.map.insert(asset, requested_margin);
        book.version = book.version.saturating_add(1);
        book.reset_sync_timer();

        Ok(requested_margin)
    }

    fn reset_sync_timer(&self) {
        if let Some(tx) = &self.sync_reset_tx {
            let _ = tx.try_send(());
        }
    }

    fn sync_request(&self) -> MarginSyncRequest {
        MarginSyncRequest {
            user: Arc::clone(&self.user),
            bot_assets: self.map.keys().cloned().collect(),
            version: self.version,
        }
    }

    async fn fetch_sync(request: &MarginSyncRequest) -> Result<(f64, Vec<AssetPosition>), Error> {
        let _permit = MARGIN_SYNC_LIMIT
            .acquire()
            .await
            .map_err(|_| Error::Custom("margin sync limiter closed".to_string()))?;

        margin_sync_timeout(
            Duration::from_secs(MARGIN_SYNC_TIMEOUT_SECS),
            request.user.get_user_margin_for_assets(&request.bot_assets),
        )
        .await
    }

    fn apply_sync(&mut self, total_on_chain: f64) {
        self.total_on_chain = total_on_chain;
        self.last_sync = Some(Instant::now());
        self.reset_sync_timer();
    }

    fn available_total(&self) -> f64 {
        self.total_on_chain - self.used()
    }

    pub fn allocate_from_current(
        &mut self,
        asset: String,
        alloc: MarginAllocation,
    ) -> Result<f64, Error> {
        let free = self.free();

        match alloc {
            MarginAllocation::Alloc(ptc) => {
                if !is_valid_margin_value(ptc) {
                    return Err(Error::InvalidMarginAmount);
                }
                let requested_margin = free * ptc;
                if requested_margin > free {
                    log::warn!("Error::InsufficientFreeMargin({})", free);
                    return Err(Error::InsufficientFreeMargin(roundf!(free, 2)));
                }
                self.map.insert(asset, requested_margin);
                self.version = self.version.saturating_add(1);
                self.reset_sync_timer();
                Ok(requested_margin)
            }

            MarginAllocation::Amount(amount) => {
                if !is_valid_margin_value(amount) {
                    return Err(Error::InvalidMarginAmount);
                }
                if amount > free {
                    log::warn!("Error::InsufficientFreeMargin({})", free);
                    return Err(Error::InsufficientFreeMargin(roundf!(free, 2)));
                }
                self.map.insert(asset, amount);
                self.version = self.version.saturating_add(1);
                self.reset_sync_timer();
                Ok(amount)
            }
        }
    }

    pub fn remove(&mut self, asset: &str) {
        if self.map.remove(asset).is_some() {
            self.version = self.version.saturating_add(1);
            self.reset_sync_timer();
        }
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn used(&self) -> f64 {
        self.map.values().copied().sum()
    }

    pub fn free(&self) -> f64 {
        self.total_on_chain - self.used()
    }

    pub fn reset(&mut self) {
        self.map.clear();
        self.last_sync = None;
        self.version = self.version.saturating_add(1);
        self.reset_sync_timer();
    }
}

pub type AssetMargin = (String, f64);

fn is_valid_margin_value(value: f64) -> bool {
    value.is_finite() && value > 0.0
}

async fn margin_sync_timeout<T, F>(duration: Duration, fut: F) -> Result<T, Error>
where
    F: Future<Output = Result<T, Error>>,
{
    tokio::time::timeout(duration, fut)
        .await
        .map_err(|_| Error::Custom("margin sync timed out".to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn margin_sync_timeout_reports_stalled_future() {
        let result = margin_sync_timeout(
            Duration::from_millis(1),
            std::future::pending::<Result<(), Error>>(),
        )
        .await;

        assert!(matches!(result, Err(Error::Custom(message)) if message.contains("timed out")));
    }

    #[test]
    fn margin_value_validation_rejects_non_finite_values() {
        assert!(is_valid_margin_value(1.0));
        assert!(!is_valid_margin_value(0.0));
        assert!(!is_valid_margin_value(f64::NAN));
        assert!(!is_valid_margin_value(f64::INFINITY));
    }
}
