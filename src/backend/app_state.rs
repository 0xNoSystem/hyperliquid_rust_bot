use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use rhai::Engine;
use sqlx::PgPool;
use tokio::sync::RwLock;
use tokio::sync::mpsc::Sender;
use uuid::Uuid;

use alloy::signers::local::PrivateKeySigner;
use hyperliquid_rust_sdk::ApproveAgent;

use super::bot_manager::BotManager;
use super::scripting::CompiledStrategy;
use crate::{IndexId, UpdateFrontend};

/// Per-user list of WebSocket senders (one per connected device).
pub type WsConnections = Arc<RwLock<HashMap<String, Vec<Sender<UpdateFrontend>>>>>;

/// In-memory nonce store: address → (nonce, created_at).
pub type NonceStore = Arc<RwLock<HashMap<String, (String, Instant)>>>;

/// Pending agent awaiting user signature.
pub struct PendingAgent {
    pub agent_signer: PrivateKeySigner,
    pub approve_agent: ApproveAgent,
    pub created_at: Instant,
}

/// In-memory store: pubkey → pending agent (one per user).
pub type PendingAgentStore = Arc<RwLock<HashMap<String, PendingAgent>>>;

/// A compiled strategy with its metadata, ready to be dispatched to a Bot/Market.
#[derive(Debug, Clone)]
pub struct CachedStrategy {
    pub compiled: CompiledStrategy,
    pub indicators: Vec<IndexId>,
    pub name: String,
}

/// Cache of compiled Rhai strategies keyed by strategy UUID.
pub type StrategyCache = Arc<RwLock<HashMap<Uuid, CachedStrategy>>>;

pub struct AppState {
    pub pool: PgPool,
    pub ws_connections: WsConnections,
    pub bot_manager: Arc<RwLock<BotManager>>,
    pub rhai_engine: Arc<Engine>,
    pub strategy_cache: StrategyCache,
    pub jwt_secret: String,
    pub encryption_key: [u8; 32],
    pub nonces: NonceStore,
    pub pending_agents: PendingAgentStore,
}

/// Send an `UpdateFrontend` message to every connected device for a given user.
pub async fn broadcast_to_user(conns: &WsConnections, pubkey: &str, msg: UpdateFrontend) {
    let conns = conns.read().await;
    if let Some(senders) = conns.get(pubkey) {
        for tx in senders {
            let _ = tx.try_send(msg.clone());
        }
    }
}
