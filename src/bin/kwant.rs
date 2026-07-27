use dotenv::dotenv;
use hyperliquid_rust_bot::{
    BaseUrl,
    backend::{
        AppState, BotManager, LocalStore, WsConnections, create_engine, create_router,
        spawn_nonce_pruner, spawn_pending_agent_pruner,
    },
    backtest::CandleStore,
    broadcast::{Broadcaster, CandleCache},
};
use log::{info, warn};
use std::collections::{HashMap, HashSet};
use std::io::{Error as IoError, ErrorKind};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

const DEFAULT_SERVER_BIND_ADDR: &str = "127.0.0.1:8090";
const DEFAULT_STORAGE_DIR: &str = "./storage";
const INFRA_TASK_SHUTDOWN_TIMEOUT_SECS: u64 = 10;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    env_logger::init();

    let server_bind_addr =
        std::env::var("SERVER_BIND_ADDR").unwrap_or_else(|_| DEFAULT_SERVER_BIND_ADDR.to_string());
    let storage_dir =
        std::env::var("STORAGE_DIR").unwrap_or_else(|_| DEFAULT_STORAGE_DIR.to_string());
    let store = Arc::new(
        LocalStore::open(&storage_dir)
            .await
            .map_err(IoError::other)?,
    );
    let encryption_key = store.encryption_key();
    let jwt_secret = match std::env::var("JWT_SECRET") {
        Ok(secret) if secret.trim().len() >= 32 => secret,
        Ok(_) => {
            return Err(IoError::new(
                ErrorKind::InvalidInput,
                "JWT_SECRET must be at least 32 bytes",
            )
            .into());
        }
        Err(_) => hex::encode(encryption_key),
    };
    info!("Opened local storage at {storage_dir}");

    // Shared infrastructure for all local wallet profiles.
    let url = BaseUrl::Mainnet;
    let infrastructure_shutdown = CancellationToken::new();
    let mut infrastructure_tasks: Vec<(&'static str, JoinHandle<()>)> = Vec::new();

    let (mut candle_cache, cache_tx) = CandleCache::new(url).await?;
    let (mut broadcaster, broadcast_tx) = Broadcaster::new(url, cache_tx.clone()).await?;
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

    let bot_manager = BotManager::new(broadcast_tx, cache_tx);
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
        store,
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
