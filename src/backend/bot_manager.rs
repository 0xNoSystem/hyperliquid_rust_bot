use std::collections::HashMap;
use std::sync::Arc;

use alloy::signers::local::PrivateKeySigner;
use log::info;
use rhai::Engine;
use sqlx::PgPool;
use tokio::sync::mpsc::Sender;
use tokio::sync::mpsc::UnboundedSender;

use super::app_state::{StrategyCache, WsConnections};
use crate::broadcast::{BroadcastCmd, CacheCmdIn};
use crate::{BaseUrl, Bot, BotEvent, Wallet};

struct BotHandle {
    cmd_tx: Sender<BotEvent>,
}

pub struct BotManager {
    bots: HashMap<String, BotHandle>,
    broadcast_tx: UnboundedSender<BroadcastCmd>,
    cache_tx: Sender<CacheCmdIn>,
}

impl BotManager {
    pub fn new(
        broadcast_tx: UnboundedSender<BroadcastCmd>,
        cache_tx: Sender<CacheCmdIn>,
    ) -> Self {
        Self {
            bots: HashMap::new(),
            broadcast_tx,
            cache_tx,
        }
    }

    /// Get the command sender for an existing bot.
    pub fn get_bot(&self, pubkey: &str) -> Option<Sender<BotEvent>> {
        self.bots.get(pubkey).map(|h| h.cmd_tx.clone())
    }

    /// Get or create a bot for the given user.
    /// Decrypts the user's API key from the DB, creates a Wallet, and spawns a Bot.
    pub async fn get_or_create_bot(
        &mut self,
        pubkey: &str,
        pool: &PgPool,
        encryption_key: &[u8; 32],
        ws_connections: WsConnections,
        rhai_engine: Arc<Engine>,
        strategy_cache: StrategyCache,
    ) -> Result<Sender<BotEvent>, crate::Error> {
        if let Some(handle) = self.bots.get(pubkey) {
            return Ok(handle.cmd_tx.clone());
        }

        // 1. Fetch user's encrypted API key from DB
        let row = sqlx::query_scalar::<_, Vec<u8>>(
            "SELECT api_key_enc FROM users WHERE pubkey = $1",
        )
        .bind(pubkey)
        .fetch_optional(pool)
        .await
        .map_err(|e| crate::Error::Custom(format!("DB error: {}", e)))?
        .ok_or_else(|| crate::Error::Custom("user has no API key set".to_string()))?;

        // 2. Decrypt
        let decrypted = super::crypto::decrypt(encryption_key, &row)
            .map_err(|e| crate::Error::Custom(format!("decryption failed: {:?}", e)))?;
        let private_key_str = String::from_utf8(decrypted)
            .map_err(|e| crate::Error::Custom(format!("invalid UTF-8 key: {}", e)))?;

        // 3. Create Wallet
        let signer: PrivateKeySigner = private_key_str
            .trim()
            .parse()
            .map_err(|e| crate::Error::Custom(format!("invalid private key: {}", e)))?;

        let url = BaseUrl::Mainnet;
        let wallet = Wallet::new(url, pubkey.to_string(), signer).await?;

        // 4. Create Bot
        let (bot, cmd_tx) =
            Bot::new(wallet, self.broadcast_tx.clone(), self.cache_tx.clone()).await?;

        // 5. Spawn bot with multi-device broadcast
        let bot_pubkey = pubkey.to_string();
        let ws_conns = ws_connections.clone();
        let pool_clone = pool.clone();
        tokio::spawn(async move {
            if let Err(e) = bot.start(ws_conns, bot_pubkey, pool_clone, rhai_engine, strategy_cache).await {
                log::error!("Bot exited with error: {:?}", e);
            }
        });

        // 6. Store handle
        self.bots.insert(pubkey.to_string(), BotHandle {
            cmd_tx: cmd_tx.clone(),
        });

        info!("Created bot for user {}", pubkey);

        Ok(cmd_tx)
    }

    /// Remove a bot for a given user (sends Kill event first).
    pub async fn remove_bot(&mut self, pubkey: &str) {
        if let Some(handle) = self.bots.remove(pubkey) {
            let _ = handle.cmd_tx.send(BotEvent::Kill).await;
            info!("Removed bot for user {}", pubkey);
        }
    }

    /// Shut down all bots gracefully.
    pub async fn shutdown_all(&mut self) {
        for (pubkey, handle) in self.bots.drain() {
            let _ = handle.cmd_tx.send(BotEvent::Kill).await;
            info!("Shut down bot for user {}", pubkey);
        }
    }
}
