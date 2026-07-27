use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rand::RngCore;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use uuid::Uuid;

use super::storage_models::{StrategyRow, StrategySummary, TradeRow};

const STORE_VERSION: u32 = 1;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct WalletRecord {
    api_key_enc: Option<String>,
    agent_valid_until: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize)]
struct WalletFile {
    version: u32,
    wallets: BTreeMap<String, WalletRecord>,
}

impl Default for WalletFile {
    fn default() -> Self {
        Self {
            version: STORE_VERSION,
            wallets: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct StrategyFile {
    version: u32,
    strategies: Vec<StrategyRow>,
}

impl Default for StrategyFile {
    fn default() -> Self {
        Self {
            version: STORE_VERSION,
            strategies: Vec::new(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct TradeFile {
    version: u32,
    trades: Vec<TradeRow>,
}

impl Default for TradeFile {
    fn default() -> Self {
        Self {
            version: STORE_VERSION,
            trades: Vec::new(),
        }
    }
}

pub struct LocalStore {
    root: PathBuf,
    encryption_key: [u8; 32],
    io_lock: Mutex<()>,
}

impl LocalStore {
    pub async fn open(root: impl Into<PathBuf>) -> Result<Self, String> {
        let root = root.into();
        tokio::fs::create_dir_all(root.join("trades"))
            .await
            .map_err(|e| format!("create local storage directory: {e}"))?;
        set_dir_permissions(&root).await?;
        set_dir_permissions(&root.join("trades")).await?;

        let encryption_key = load_or_create_master_key(&root.join("master.key")).await?;
        let store = Self {
            root,
            encryption_key,
            io_lock: Mutex::new(()),
        };

        store
            .ensure_file::<WalletFile>(&store.wallets_path())
            .await?;
        store
            .ensure_file::<StrategyFile>(&store.strategies_path())
            .await?;
        validate_version(
            read_json::<WalletFile>(&store.wallets_path())
                .await?
                .version,
        )?;
        validate_version(
            read_json::<StrategyFile>(&store.strategies_path())
                .await?
                .version,
        )?;
        Ok(store)
    }

    pub fn encryption_key(&self) -> [u8; 32] {
        self.encryption_key
    }

    pub async fn is_ready(&self) -> bool {
        self.wallets_path().is_file() && self.strategies_path().is_file()
    }

    pub async fn ensure_wallet(&self, pubkey: &str) -> Result<(), String> {
        let _guard = self.io_lock.lock().await;
        let path = self.wallets_path();
        let mut data: WalletFile = read_json(&path).await?;
        validate_version(data.version)?;
        data.wallets.entry(pubkey.to_string()).or_default();
        write_json_atomic(&path, &data).await
    }

    pub async fn encrypted_agent_key(&self, pubkey: &str) -> Result<Option<Vec<u8>>, String> {
        let _guard = self.io_lock.lock().await;
        let data: WalletFile = read_json(&self.wallets_path()).await?;
        validate_version(data.version)?;
        data.wallets
            .get(pubkey)
            .and_then(|wallet| wallet.api_key_enc.as_deref())
            .map(|encoded| {
                hex::decode(encoded).map_err(|e| format!("decode stored agent key: {e}"))
            })
            .transpose()
    }

    pub async fn set_encrypted_agent_key(
        &self,
        pubkey: &str,
        encrypted: &[u8],
        valid_until: i64,
    ) -> Result<(), String> {
        let _guard = self.io_lock.lock().await;
        let path = self.wallets_path();
        let mut data: WalletFile = read_json(&path).await?;
        validate_version(data.version)?;
        let wallet = data.wallets.entry(pubkey.to_string()).or_default();
        wallet.api_key_enc = Some(hex::encode(encrypted));
        wallet.agent_valid_until = Some(valid_until);
        write_json_atomic(&path, &data).await
    }

    pub async fn list_strategies(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<StrategySummary>, String> {
        let _guard = self.io_lock.lock().await;
        let mut data: StrategyFile = read_json(&self.strategies_path()).await?;
        validate_version(data.version)?;
        data.strategies
            .sort_by_key(|strategy| std::cmp::Reverse(strategy.updated_at));
        Ok(data
            .strategies
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .map(|row| StrategySummary {
                id: row.id,
                name: row.name,
                is_active: row.is_active,
            })
            .collect())
    }

    pub async fn strategy(&self, id: Uuid) -> Result<Option<StrategyRow>, String> {
        let _guard = self.io_lock.lock().await;
        let data: StrategyFile = read_json(&self.strategies_path()).await?;
        validate_version(data.version)?;
        Ok(data.strategies.into_iter().find(|row| row.id == id))
    }

    pub async fn insert_strategy(&self, row: StrategyRow) -> Result<StrategyRow, String> {
        let _guard = self.io_lock.lock().await;
        let path = self.strategies_path();
        let mut data: StrategyFile = read_json(&path).await?;
        validate_version(data.version)?;
        data.strategies.push(row.clone());
        write_json_atomic(&path, &data).await?;
        Ok(row)
    }

    pub async fn update_strategy(&self, row: StrategyRow) -> Result<Option<StrategyRow>, String> {
        let _guard = self.io_lock.lock().await;
        let path = self.strategies_path();
        let mut data: StrategyFile = read_json(&path).await?;
        validate_version(data.version)?;
        let Some(existing) = data.strategies.iter_mut().find(|item| item.id == row.id) else {
            return Ok(None);
        };
        *existing = row.clone();
        write_json_atomic(&path, &data).await?;
        Ok(Some(row))
    }

    pub async fn delete_strategy(&self, id: Uuid) -> Result<bool, String> {
        let _guard = self.io_lock.lock().await;
        let path = self.strategies_path();
        let mut data: StrategyFile = read_json(&path).await?;
        validate_version(data.version)?;
        let old_len = data.strategies.len();
        data.strategies.retain(|row| row.id != id);
        if data.strategies.len() == old_len {
            return Ok(false);
        }
        write_json_atomic(&path, &data).await?;
        Ok(true)
    }

    pub async fn append_trade(&self, pubkey: &str, row: TradeRow) -> Result<(), String> {
        let _guard = self.io_lock.lock().await;
        let path = self.trade_path(pubkey)?;
        let mut data = if path.exists() {
            read_json::<TradeFile>(&path).await?
        } else {
            TradeFile::default()
        };
        validate_version(data.version)?;
        data.trades.push(row);
        write_json_atomic(&path, &data).await
    }

    pub async fn list_trades(
        &self,
        pubkey: &str,
        market: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<TradeRow>, String> {
        let _guard = self.io_lock.lock().await;
        let path = self.trade_path(pubkey)?;
        if !path.exists() {
            return Ok(Vec::new());
        }
        let mut data: TradeFile = read_json(&path).await?;
        validate_version(data.version)?;
        data.trades.retain(|trade| trade.market == market);
        data.trades
            .sort_by_key(|trade| std::cmp::Reverse(trade.close_time));
        Ok(data
            .trades
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .collect())
    }

    fn wallets_path(&self) -> PathBuf {
        self.root.join("wallets.json")
    }

    fn strategies_path(&self) -> PathBuf {
        self.root.join("strategies.json")
    }

    fn trade_path(&self, pubkey: &str) -> Result<PathBuf, String> {
        let key = pubkey.strip_prefix("0x").unwrap_or(pubkey);
        if key.is_empty() || !key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("invalid wallet key for local trade storage".to_string());
        }
        Ok(self
            .root
            .join("trades")
            .join(format!("{}.json", key.to_ascii_lowercase())))
    }

    async fn ensure_file<T>(&self, path: &Path) -> Result<(), String>
    where
        T: Default + Serialize,
    {
        if !path.exists() {
            write_json_atomic(path, &T::default()).await?;
        }
        Ok(())
    }
}

async fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("parse {}: {e}", path.display()))
}

fn validate_version(version: u32) -> Result<(), String> {
    if version == STORE_VERSION {
        Ok(())
    } else {
        Err(format!(
            "unsupported local storage version {version}; expected {STORE_VERSION}"
        ))
    }
}

async fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|e| format!("serialize {}: {e}", path.display()))?;
    let tmp = path.with_extension("json.tmp");
    let mut file = tokio::fs::File::create(&tmp)
        .await
        .map_err(|e| format!("create {}: {e}", tmp.display()))?;
    file.write_all(&bytes)
        .await
        .map_err(|e| format!("write {}: {e}", tmp.display()))?;
    file.sync_all()
        .await
        .map_err(|e| format!("sync {}: {e}", tmp.display()))?;
    drop(file);
    set_file_permissions(&tmp).await?;
    tokio::fs::rename(&tmp, path)
        .await
        .map_err(|e| format!("replace {}: {e}", path.display()))?;
    Ok(())
}

async fn load_or_create_master_key(path: &Path) -> Result<[u8; 32], String> {
    if path.exists() {
        let raw = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| format!("read {}: {e}", path.display()))?;
        let bytes =
            hex::decode(raw.trim()).map_err(|e| format!("decode {}: {e}", path.display()))?;
        return bytes
            .try_into()
            .map_err(|_| format!("{} must contain a 32-byte hex key", path.display()));
    }

    let mut key = [0_u8; 32];
    rand::thread_rng().fill_bytes(&mut key);
    tokio::fs::write(path, hex::encode(key))
        .await
        .map_err(|e| format!("create {}: {e}", path.display()))?;
    set_file_permissions(path).await?;
    Ok(key)
}

#[cfg(unix)]
async fn set_file_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .await
        .map_err(|e| format!("set permissions on {}: {e}", path.display()))
}

#[cfg(not(unix))]
async fn set_file_permissions(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
async fn set_dir_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .await
        .map_err(|e| format!("set permissions on {}: {e}", path.display()))
}

#[cfg(not(unix))]
async fn set_dir_permissions(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_strategy(name: &str) -> StrategyRow {
        let now = chrono::Utc::now();
        StrategyRow {
            id: Uuid::new_v4(),
            name: name.to_string(),
            on_idle: String::new(),
            on_open: String::new(),
            on_busy: String::new(),
            indicators: serde_json::json!([]),
            state_declarations: None,
            is_active: Some(false),
            created_at: Some(now),
            updated_at: Some(now),
        }
    }

    async fn test_store() -> (PathBuf, LocalStore) {
        let root = std::env::temp_dir().join(format!("kwant-store-test-{}", Uuid::new_v4()));
        let store = LocalStore::open(&root).await.expect("store should open");
        (root, store)
    }

    #[tokio::test]
    async fn strategies_are_global() {
        let (root, store) = test_store().await;
        store.ensure_wallet("0xaaaa").await.unwrap();
        store.ensure_wallet("0xbbbb").await.unwrap();
        let strategy = test_strategy("shared");
        store.insert_strategy(strategy.clone()).await.unwrap();

        assert_eq!(
            store.strategy(strategy.id).await.unwrap().unwrap().name,
            "shared"
        );
        assert_eq!(store.list_strategies(10, 0).await.unwrap().len(), 1);
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn agent_keys_are_wallet_scoped_and_survive_reopen() {
        let (root, store) = test_store().await;
        store.ensure_wallet("0xaaaa").await.unwrap();
        store
            .set_encrypted_agent_key("0xaaaa", &[1, 2, 3], 123)
            .await
            .unwrap();
        drop(store);

        let reopened = LocalStore::open(&root).await.unwrap();
        assert_eq!(
            reopened.encrypted_agent_key("0xaaaa").await.unwrap(),
            Some(vec![1, 2, 3])
        );
        assert_eq!(reopened.encrypted_agent_key("0xbbbb").await.unwrap(), None);
        tokio::fs::remove_dir_all(root).await.unwrap();
    }

    #[tokio::test]
    async fn trades_are_wallet_scoped_sorted_and_paginated() {
        let (root, store) = test_store().await;
        for (wallet, close_time) in [("0xaaaa", 10), ("0xaaaa", 20), ("0xbbbb", 30)] {
            store
                .append_trade(
                    wallet,
                    TradeRow {
                        id: Uuid::new_v4(),
                        pubkey: wallet.to_string(),
                        market: "BTC".to_string(),
                        side: "Long".to_string(),
                        size: 1.0,
                        pnl: 1.0,
                        total_pnl: 1.0,
                        fees: 0.0,
                        funding: 0.0,
                        open_time: 1,
                        open_price: 1.0,
                        open_type: "Market".to_string(),
                        close_time,
                        close_price: 2.0,
                        close_type: "Market".to_string(),
                        strategy: Some("shared".to_string()),
                    },
                )
                .await
                .unwrap();
        }

        let page = store.list_trades("0xaaaa", "BTC", 1, 0).await.unwrap();
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].close_time, 20);
        assert_eq!(
            store
                .list_trades("0xbbbb", "BTC", 10, 0)
                .await
                .unwrap()
                .len(),
            1
        );
        tokio::fs::remove_dir_all(root).await.unwrap();
    }
}
