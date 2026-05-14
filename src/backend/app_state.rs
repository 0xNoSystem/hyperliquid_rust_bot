use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use rhai::Engine;
use sqlx::PgPool;
use tokio::sync::RwLock;
use tokio::sync::mpsc::{Sender, error::TrySendError};
use uuid::Uuid;

use alloy::signers::local::PrivateKeySigner;
use hyperliquid_rust_sdk::ApproveAgent;

use super::bot_manager::BotManager;
use super::scripting::{CompiledStrategy, StateDeclarations};
use crate::backtest::CandleStore;
use crate::metrics;
use crate::{IndexId, UpdateFrontend};

/// Per-user list of WebSocket senders (one per connected device).
pub type WsConnections = Arc<RwLock<HashMap<String, Vec<WsConnection>>>>;

#[derive(Clone)]
pub struct WsConnection {
    pub id: Uuid,
    pub tx: Sender<UpdateFrontend>,
}

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

/// Users with an in-flight bot startup.
pub type BotStartupStore = Arc<RwLock<HashSet<String>>>;

/// A compiled strategy with its metadata, ready to be dispatched to a Bot/Market.
#[derive(Debug, Clone)]
pub struct CachedStrategy {
    pub compiled: CompiledStrategy,
    pub indicators: Vec<IndexId>,
    pub state_declarations: Option<StateDeclarations>,
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
    pub candle_store: Arc<CandleStore>,
    pub active_backtests: Arc<RwLock<HashSet<String>>>,
    pub bot_startups: BotStartupStore,
    pub jwt_secret: String,
    pub encryption_key: [u8; 32],
    pub nonces: NonceStore,
    pub pending_agents: PendingAgentStore,
}

/// Send an `UpdateFrontend` message to every connected device for a given user.
pub async fn broadcast_to_user(conns: &WsConnections, pubkey: &str, msg: UpdateFrontend) {
    let Some(senders) = ({
        let conns = conns.read().await;
        conns.get(pubkey).cloned()
    }) else {
        return;
    };

    let mut stale = HashSet::new();
    for conn in senders {
        match conn.tx.try_send(msg.clone()) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                metrics::inc_frontend_ws_dropped();
                log::warn!("dropping slow websocket connection for user {pubkey}");
                stale.insert(conn.id);
            }
            Err(TrySendError::Closed(_)) => {
                stale.insert(conn.id);
            }
        };
    }

    if stale.is_empty() {
        return;
    }

    let mut conns = conns.write().await;
    let mut remove_user = false;

    if let Some(senders) = conns.get_mut(pubkey) {
        senders.retain(|conn| !stale.contains(&conn.id));
        remove_user = senders.is_empty();
    }

    if remove_user {
        conns.remove(pubkey);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    fn user_error(text: &str) -> UpdateFrontend {
        UpdateFrontend::UserError(text.to_string())
    }

    #[tokio::test]
    async fn broadcast_to_user_prunes_full_connection_and_keeps_healthy_one() {
        let conns: WsConnections = Arc::new(RwLock::new(HashMap::new()));
        let (full_tx, mut full_rx) = mpsc::channel(1);
        let (healthy_tx, mut healthy_rx) = mpsc::channel(1);

        full_tx
            .try_send(user_error("queued"))
            .expect("test queue should accept initial message");

        conns.write().await.insert(
            "user".to_string(),
            vec![
                WsConnection {
                    id: Uuid::new_v4(),
                    tx: full_tx,
                },
                WsConnection {
                    id: Uuid::new_v4(),
                    tx: healthy_tx,
                },
            ],
        );

        broadcast_to_user(&conns, "user", user_error("live")).await;

        let guard = conns.read().await;
        assert_eq!(guard.get("user").map(Vec::len), Some(1));
        drop(guard);

        assert!(matches!(
            full_rx.recv().await,
            Some(UpdateFrontend::UserError(msg)) if msg == "queued"
        ));
        assert!(matches!(
            healthy_rx.recv().await,
            Some(UpdateFrontend::UserError(msg)) if msg == "live"
        ));
    }

    #[tokio::test]
    async fn broadcast_to_user_removes_user_when_all_connections_closed() {
        let conns: WsConnections = Arc::new(RwLock::new(HashMap::new()));
        let (tx, rx) = mpsc::channel(1);
        drop(rx);

        conns.write().await.insert(
            "user".to_string(),
            vec![WsConnection {
                id: Uuid::new_v4(),
                tx,
            }],
        );

        broadcast_to_user(&conns, "user", user_error("live")).await;

        assert!(!conns.read().await.contains_key("user"));
    }
}
