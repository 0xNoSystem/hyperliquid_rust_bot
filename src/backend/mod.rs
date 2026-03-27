pub(crate) mod app_state;
pub(crate) mod auth;
pub(crate) mod bot_manager;
pub(crate) mod crypto;
pub(crate) mod db;
pub(crate) mod routes;
pub(crate) mod scripting;

// Re-exports for the binary crate
pub use app_state::{
    AppState, CachedStrategy, NonceStore, StrategyCache, WsConnections, broadcast_to_user,
};
pub use auth::{AuthUser, spawn_nonce_pruner, spawn_pending_agent_pruner};
pub use bot_manager::BotManager;
pub use db::{StrategyRow, TradeRow};
pub use routes::create_router;
pub use scripting::{CompiledStrategy, compile_strategy, create_engine};
