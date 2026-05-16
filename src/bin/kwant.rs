use dotenv::dotenv;
use hyperliquid_rust_bot::{
    BaseUrl,
    backend::{
        AppState, BotManager, WsConnections, create_engine, create_router, spawn_nonce_pruner,
        spawn_pending_agent_pruner,
    },
    backtest::CandleStore,
    broadcast::{Broadcaster, CandleCache, UserEventRelay},
};
use log::{info, warn};
use sqlx::postgres::PgPoolOptions;
use std::collections::{HashMap, HashSet};
use std::io::{Error as IoError, ErrorKind};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

const DEFAULT_DATABASE_MAX_CONNECTIONS: u32 = 10;
const DEFAULT_DATABASE_CONNECT_TIMEOUT_SECS: u64 = 10;
const DEFAULT_DATABASE_ACQUIRE_TIMEOUT_SECS: u64 = 5;
const DEFAULT_SERVER_BIND_ADDR: &str = "0.0.0.0:8090";
const JWT_SECRET_MIN_BYTES: usize = 32;
const INFRA_TASK_SHUTDOWN_TIMEOUT_SECS: u64 = 10;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    env_logger::init();

    let database_url = std::env::var("DATABASE_URL")
        .map_err(|_| IoError::new(ErrorKind::InvalidInput, "DATABASE_URL required"))?;
    let jwt_secret = validate_jwt_secret(
        std::env::var("JWT_SECRET")
            .map_err(|_| IoError::new(ErrorKind::InvalidInput, "JWT_SECRET required"))?,
    )?;
    let encryption_key_hex = std::env::var("ENCRYPTION_KEY")
        .map_err(|_| IoError::new(ErrorKind::InvalidInput, "ENCRYPTION_KEY required"))?;
    let encryption_key_vec = hex::decode(&encryption_key_hex)
        .map_err(|_| IoError::new(ErrorKind::InvalidInput, "invalid ENCRYPTION_KEY hex"))?;
    let encryption_key: [u8; 32] = encryption_key_vec.try_into().map_err(|_| {
        IoError::new(
            ErrorKind::InvalidInput,
            "ENCRYPTION_KEY must decode to 32 bytes",
        )
    })?;
    let database_max_connections = parse_optional_u32_env(
        "DATABASE_MAX_CONNECTIONS",
        std::env::var("DATABASE_MAX_CONNECTIONS").ok(),
        DEFAULT_DATABASE_MAX_CONNECTIONS,
    )?;
    let database_connect_timeout_secs = parse_optional_u64_env(
        "DATABASE_CONNECT_TIMEOUT_SECONDS",
        std::env::var("DATABASE_CONNECT_TIMEOUT_SECONDS").ok(),
        DEFAULT_DATABASE_CONNECT_TIMEOUT_SECS,
    )?;
    let database_acquire_timeout_secs = parse_optional_u64_env(
        "DATABASE_ACQUIRE_TIMEOUT_SECONDS",
        std::env::var("DATABASE_ACQUIRE_TIMEOUT_SECONDS").ok(),
        DEFAULT_DATABASE_ACQUIRE_TIMEOUT_SECS,
    )?;
    let server_bind_addr =
        std::env::var("SERVER_BIND_ADDR").unwrap_or_else(|_| DEFAULT_SERVER_BIND_ADDR.to_string());

    let pool_options = PgPoolOptions::new()
        .max_connections(database_max_connections)
        .acquire_timeout(Duration::from_secs(database_acquire_timeout_secs));
    let pool = tokio::time::timeout(
        Duration::from_secs(database_connect_timeout_secs),
        pool_options.connect(&database_url),
    )
    .await
    .map_err(|_| IoError::new(ErrorKind::TimedOut, "DATABASE_URL connect timed out"))??;

    info!("Connected to Supabase PostgreSQL");

    // Shared infrastructure (one instance, all users)
    let url = BaseUrl::Mainnet;
    let infrastructure_shutdown = CancellationToken::new();
    let mut infrastructure_tasks: Vec<(&'static str, JoinHandle<()>)> = Vec::new();

    let (mut candle_cache, cache_tx) = CandleCache::new(url).await?;
    let (mut broadcaster, broadcast_tx) = Broadcaster::new(url, cache_tx.clone()).await?;
    let user_event_tx = match UserEventRelay::from_env(url).await? {
        Some((mut relay, tx)) => {
            let shutdown = infrastructure_shutdown.child_token();
            infrastructure_tasks.push((
                "user_event_relay",
                tokio::spawn(async move { relay.start(shutdown).await }),
            ));
            Some(tx)
        }
        None => None,
    };
    let shutdown = infrastructure_shutdown.child_token();
    infrastructure_tasks.push((
        "candle_cache",
        tokio::spawn(async move { candle_cache.start(shutdown).await }),
    ));
    let shutdown = infrastructure_shutdown.child_token();
    infrastructure_tasks.push((
        "broadcaster",
        tokio::spawn(async move { broadcaster.start(shutdown).await }),
    ));

    let bot_manager = BotManager::new(broadcast_tx, cache_tx, user_event_tx);
    let rhai_engine = Arc::new(create_engine());

    let ws_connections: WsConnections = Arc::new(RwLock::new(HashMap::new()));
    let nonces = Arc::new(RwLock::new(HashMap::new()));
    let pending_agents = Arc::new(RwLock::new(HashMap::new()));
    let pending_builder_fee_approvals = Arc::new(RwLock::new(HashMap::new()));

    // Spawn pruners
    infrastructure_tasks.push((
        "nonce_pruner",
        spawn_nonce_pruner(nonces.clone(), infrastructure_shutdown.child_token()),
    ));
    infrastructure_tasks.push((
        "pending_agent_pruner",
        spawn_pending_agent_pruner(
            pending_agents.clone(),
            infrastructure_shutdown.child_token(),
        ),
    ));

    let candle_store = Arc::new(CandleStore::open("./data/candles")?);

    let state = Arc::new(AppState {
        pool,
        ws_connections,
        bot_manager: Arc::new(RwLock::new(bot_manager)),
        rhai_engine,
        strategy_cache: Arc::new(RwLock::new(HashMap::new())),
        candle_store,
        active_backtests: Arc::new(RwLock::new(HashSet::new())),
        bot_startups: Arc::new(RwLock::new(HashSet::new())),
        jwt_secret,
        encryption_key,
        nonces,
        pending_agents,
        pending_builder_fee_approvals,
    });

    let app = create_router(Arc::clone(&state));

    info!("Starting server on {server_bind_addr}");
    let listener = tokio::net::TcpListener::bind(&server_bind_addr).await?;
    let server_result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await;

    info!("Server stopped; shutting down active bots");
    let shutdown_senders = state.bot_manager.write().await.drain_shutdown_senders();
    BotManager::shutdown_senders(shutdown_senders).await;

    info!("Stopping shared infrastructure");
    infrastructure_shutdown.cancel();
    drop(state);
    join_infrastructure_tasks(infrastructure_tasks).await;

    server_result?;

    Ok(())
}

async fn join_infrastructure_tasks(tasks: Vec<(&'static str, JoinHandle<()>)>) {
    for (name, mut task) in tasks {
        match tokio::time::timeout(
            Duration::from_secs(INFRA_TASK_SHUTDOWN_TIMEOUT_SECS),
            &mut task,
        )
        .await
        {
            Ok(Ok(())) => info!("{name} stopped"),
            Ok(Err(err)) if err.is_cancelled() => info!("{name} aborted"),
            Ok(Err(err)) => warn!("{name} failed during shutdown: {err}"),
            Err(_) => {
                warn!(
                    "{name} did not stop within {INFRA_TASK_SHUTDOWN_TIMEOUT_SECS}s; aborting task"
                );
                task.abort();
                if let Err(err) = task.await
                    && !err.is_cancelled()
                {
                    warn!("{name} failed after abort: {err}");
                }
            }
        }
    }
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("Shutdown signal received: Ctrl-C");
            }
            _ = terminate_signal() => {
                info!("Shutdown signal received: SIGTERM");
            }
        }
    }

    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await.ok();
        info!("Shutdown signal received");
    }
}

#[cfg(unix)]
async fn terminate_signal() {
    match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
        Ok(mut signal) => {
            signal.recv().await;
        }
        Err(err) => {
            log::warn!("failed to install SIGTERM handler: {err}");
            std::future::pending::<()>().await;
        }
    }
}

fn parse_optional_u32_env(key: &str, value: Option<String>, default: u32) -> Result<u32, IoError> {
    let Some(value) = value else {
        return Ok(default);
    };

    let parsed = value.parse::<u32>().map_err(|_| {
        IoError::new(
            ErrorKind::InvalidInput,
            format!("{key} must be a positive integer"),
        )
    })?;

    if parsed == 0 {
        return Err(IoError::new(
            ErrorKind::InvalidInput,
            format!("{key} must be greater than zero"),
        ));
    }

    Ok(parsed)
}

fn parse_optional_u64_env(key: &str, value: Option<String>, default: u64) -> Result<u64, IoError> {
    let Some(value) = value else {
        return Ok(default);
    };

    let parsed = value.parse::<u64>().map_err(|_| {
        IoError::new(
            ErrorKind::InvalidInput,
            format!("{key} must be a positive integer"),
        )
    })?;

    if parsed == 0 {
        return Err(IoError::new(
            ErrorKind::InvalidInput,
            format!("{key} must be greater than zero"),
        ));
    }

    Ok(parsed)
}

fn validate_jwt_secret(secret: String) -> Result<String, IoError> {
    if secret.trim().len() < JWT_SECRET_MIN_BYTES {
        return Err(IoError::new(
            ErrorKind::InvalidInput,
            format!("JWT_SECRET must be at least {JWT_SECRET_MIN_BYTES} bytes"),
        ));
    }

    Ok(secret)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_optional_u32_env_uses_default_when_missing() {
        assert_eq!(
            parse_optional_u32_env("TEST_VALUE", None, 10).expect("default should parse"),
            10
        );
    }

    #[test]
    fn parse_optional_u32_env_rejects_zero_and_invalid_values() {
        assert!(parse_optional_u32_env("TEST_VALUE", Some("0".to_string()), 10).is_err());
        assert!(parse_optional_u32_env("TEST_VALUE", Some("nope".to_string()), 10).is_err());
    }

    #[test]
    fn parse_optional_u64_env_rejects_zero_and_invalid_values() {
        assert_eq!(
            parse_optional_u64_env("TEST_VALUE", None, 10).expect("default should parse"),
            10
        );
        assert!(parse_optional_u64_env("TEST_VALUE", Some("0".to_string()), 10).is_err());
        assert!(parse_optional_u64_env("TEST_VALUE", Some("nope".to_string()), 10).is_err());
    }

    #[test]
    fn validate_jwt_secret_requires_minimum_length() {
        assert!(validate_jwt_secret("short".to_string()).is_err());
        assert!(validate_jwt_secret("x".repeat(JWT_SECRET_MIN_BYTES)).is_ok());
    }
}
