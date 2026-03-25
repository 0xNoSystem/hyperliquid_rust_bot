use dotenv::dotenv;
use hyperliquid_rust_bot::{
    BaseUrl,
    backend::{AppState, BotManager, WsConnections, create_engine, create_router, spawn_nonce_pruner},
    broadcast::{Broadcaster, CandleCache},
};
use log::info;
use sqlx::postgres::PgPoolOptions;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    env_logger::init();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL required");
    let jwt_secret = std::env::var("JWT_SECRET").expect("JWT_SECRET required");
    let encryption_key_hex = std::env::var("ENCRYPTION_KEY").expect("ENCRYPTION_KEY required");
    let encryption_key = hex::decode(&encryption_key_hex).expect("invalid hex key");
    assert_eq!(encryption_key.len(), 32, "ENCRYPTION_KEY must be 32 bytes");

    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await?;

    info!("Connected to Supabase PostgreSQL");

    // Shared infrastructure (one instance, all users)
    let url = BaseUrl::Mainnet;
    let (mut candle_cache, cache_tx) = CandleCache::new(url).await?;
    let (mut broadcaster, broadcast_tx) = Broadcaster::new(url, cache_tx.clone()).await?;
    tokio::spawn(async move { candle_cache.start().await });
    tokio::spawn(async move { broadcaster.start().await });

    let bot_manager = BotManager::new(broadcast_tx, cache_tx);
    let rhai_engine = Arc::new(create_engine());

    let ws_connections: WsConnections = Arc::new(RwLock::new(HashMap::new()));
    let nonces = Arc::new(RwLock::new(HashMap::new()));

    // Spawn nonce pruner
    spawn_nonce_pruner(nonces.clone());

    let state = Arc::new(AppState {
        pool,
        ws_connections,
        bot_manager: Arc::new(RwLock::new(bot_manager)),
        rhai_engine,
        strategy_cache: Arc::new(RwLock::new(HashMap::new())),
        jwt_secret,
        encryption_key: encryption_key.try_into().unwrap(),
        nonces,
    });

    let app = create_router(state);

    info!("Starting server on 0.0.0.0:8090");
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8090").await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c().await.ok();
    info!("Shutdown signal received");
}
