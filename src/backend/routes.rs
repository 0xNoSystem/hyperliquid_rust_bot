use std::sync::Arc;
use std::time::Instant;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Path, Query, State, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use log::info;
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use super::app_state::{AppState, CachedStrategy, broadcast_to_user};
use super::auth::{self, AuthUser};
use crate::backtest::BacktestRunRequest;
use crate::{
    BacktestProgressUpdate, BacktestResultUpdate, BacktestRunError, BacktestRunPayload,
    BacktestRunResponse, Backtester, BotEvent, UpdateFrontend, get_time_now,
};

pub fn create_router(state: Arc<AppState>) -> Router {
    Router::new()
        // Auth (unauthenticated)
        .route("/auth/nonce", get(get_nonce))
        .route("/auth/verify", post(verify_signature))
        // Bot commands (authenticated)
        .route("/command", post(execute_command))
        .route("/backtest", post(run_backtest))
        // Data queries (authenticated)
        .route("/trades/{market}", get(get_trades))
        .route(
            "/strategies",
            get(list_strategies).post(save_strategy),
        )
        .route(
            "/strategies/{id}",
            put(update_strategy).delete(delete_strategy),
        )
        .route("/api-key", post(set_api_key))
        // WebSocket
        .route("/ws", get(ws_handler))
        // Middleware
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

// ── Auth Routes ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct NonceQuery {
    address: String,
}

#[derive(Serialize)]
struct NonceResponse {
    nonce: String,
}

async fn get_nonce(
    State(state): State<Arc<AppState>>,
    Query(params): Query<NonceQuery>,
) -> impl IntoResponse {
    let address = params.address.to_lowercase();
    let nonce = auth::generate_nonce();

    {
        let mut store = state.nonces.write().await;
        store.insert(address, (nonce.clone(), Instant::now()));
    }

    Json(NonceResponse { nonce })
}

#[derive(Deserialize)]
struct VerifyPayload {
    address: String,
    signature: String,
    nonce: String,
}

#[derive(Serialize)]
struct VerifyResponse {
    token: String,
}

async fn verify_signature(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<VerifyPayload>,
) -> Result<Json<VerifyResponse>, StatusCode> {
    let address = payload.address.to_lowercase();

    // Check nonce
    let stored_nonce = {
        let mut store = state.nonces.write().await;
        store.remove(&address)
    };

    let (expected_nonce, created_at) = stored_nonce.ok_or(StatusCode::BAD_REQUEST)?;

    // Check nonce hasn't expired (5 minutes)
    if created_at.elapsed().as_secs() > 300 {
        return Err(StatusCode::GONE);
    }

    if payload.nonce != expected_nonce {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Verify signature
    auth::verify_signature(&address, &payload.signature, &payload.nonce)
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    // Upsert user in DB
    sqlx::query("INSERT INTO users (pubkey) VALUES ($1) ON CONFLICT (pubkey) DO NOTHING")
        .bind(&address)
        .execute(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Issue JWT
    let token =
        auth::issue_jwt(&address, &state.jwt_secret).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(VerifyResponse { token }))
}

// ── Command Route ────────────────────────────────────────────────────────────

async fn execute_command(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(event): Json<BotEvent>,
) -> impl IntoResponse {
    let manager = state.bot_manager.read().await;
    if let Some(cmd_tx) = manager.get_bot(&auth.pubkey) {
        match cmd_tx.try_send(event) {
            Ok(()) => StatusCode::OK,
            Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => StatusCode::TOO_MANY_REQUESTS,
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                StatusCode::SERVICE_UNAVAILABLE
            }
        }
    } else {
        StatusCode::NOT_FOUND
    }
}

// ── Backtest Route ───────────────────────────────────────────────────────────

fn make_backtest_run_id(asset: &str) -> String {
    format!("bt-{}-{}", asset.to_lowercase(), get_time_now())
}

fn validate_backtest_request(request: &BacktestRunRequest) -> Result<(), String> {
    let cfg = &request.config;
    if cfg.asset.trim().is_empty() {
        return Err("asset must not be empty".to_string());
    }
    if !cfg.margin.is_finite() || cfg.margin <= 0.0 {
        return Err("margin must be a positive finite number".to_string());
    }
    if cfg.lev == 0 {
        return Err("lev must be greater than zero".to_string());
    }
    if cfg.end_time <= cfg.start_time {
        return Err("endTime must be greater than startTime".to_string());
    }
    if cfg.resolution.to_millis() == 0 {
        return Err("resolution must be a supported timeframe".to_string());
    }
    Ok(())
}

async fn run_backtest(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(payload): Json<BacktestRunPayload>,
) -> impl IntoResponse {
    let mut request: BacktestRunRequest = payload.into();
    let run_id = request
        .run_id
        .clone()
        .filter(|id| !id.trim().is_empty())
        .unwrap_or_else(|| make_backtest_run_id(&request.config.asset));

    if let Err(message) = validate_backtest_request(&request) {
        return Json(serde_json::to_value(BacktestRunError {
            run_id,
            message,
            progress: Vec::new(),
        })
        .unwrap())
        .into_response();
    }

    request.run_id = Some(run_id.clone());
    let mut backtester = Backtester::from_request(request);
    let mut progress = Vec::new();

    let ws_conns = state.ws_connections.clone();
    let pubkey = auth.pubkey.clone();
    let progress_run_id = run_id.clone();

    match backtester
        .run_with_progress(|evt| {
            progress.push(evt.clone());
            let conns = ws_conns.clone();
            let pk = pubkey.clone();
            let rid = progress_run_id.clone();
            let evt = evt.clone();
            tokio::spawn(async move {
                broadcast_to_user(
                    &conns,
                    &pk,
                    UpdateFrontend::BacktestProgress(BacktestProgressUpdate {
                        run_id: rid,
                        progress: evt,
                    }),
                )
                .await;
            });
        })
        .await
    {
        Ok(mut result) => {
            result.run_id = run_id.clone();

            let ws_conns = state.ws_connections.clone();
            let pk = auth.pubkey.clone();
            let rid = run_id.clone();
            let res_clone = result.clone();
            tokio::spawn(async move {
                broadcast_to_user(
                    &ws_conns,
                    &pk,
                    UpdateFrontend::BacktestResult(Box::new(BacktestResultUpdate {
                        run_id: rid,
                        result: res_clone,
                    })),
                )
                .await;
            });

            Json(
                serde_json::to_value(BacktestRunResponse {
                    run_id,
                    result,
                    progress,
                })
                .unwrap(),
            )
            .into_response()
        }
        Err(err) => Json(
            serde_json::to_value(BacktestRunError {
                run_id,
                message: err.to_string(),
                progress,
            })
            .unwrap(),
        )
        .into_response(),
    }
}

// ── Trades Route ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct TradeQueryParams {
    limit: Option<i64>,
    offset: Option<i64>,
}

async fn get_trades(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(market): Path<String>,
    Query(params): Query<TradeQueryParams>,
) -> Result<impl IntoResponse, StatusCode> {
    let limit = params.limit.unwrap_or(50);
    let offset = params.offset.unwrap_or(0);

    let rows = sqlx::query_as::<_, super::db::TradeRow>(
        "SELECT * FROM trades WHERE pubkey = $1 AND market = $2 ORDER BY close_time DESC LIMIT $3 OFFSET $4",
    )
    .bind(&auth.pubkey)
    .bind(&market)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(rows))
}

// ── Strategies Routes ────────────────────────────────────────────────────────

async fn list_strategies(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> Result<impl IntoResponse, StatusCode> {
    let rows = sqlx::query_as::<_, super::db::StrategyRow>(
        "SELECT * FROM strategies WHERE pubkey = $1 ORDER BY updated_at DESC",
    )
    .bind(&auth.pubkey)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(rows))
}

#[derive(Deserialize)]
struct SaveStrategyPayload {
    name: String,
    on_idle: String,
    on_open: String,
    on_busy: String,
    indicators: serde_json::Value,
    is_active: Option<bool>,
}

async fn save_strategy(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(payload): Json<SaveStrategyPayload>,
) -> Result<impl IntoResponse, StatusCode> {
    // Validate scripts compile before persisting
    let compiled = match super::scripting::compile_strategy(
        &state.rhai_engine,
        &payload.on_idle,
        &payload.on_open,
        &payload.on_busy,
    ) {
        Ok(c) => c,
        Err(msg) => {
            return Ok((StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": msg }))).into_response());
        }
    };

    let row = sqlx::query_as::<_, super::db::StrategyRow>(
        "INSERT INTO strategies (pubkey, name, on_idle, on_open, on_busy, indicators, is_active)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING *",
    )
    .bind(&auth.pubkey)
    .bind(&payload.name)
    .bind(&payload.on_idle)
    .bind(&payload.on_open)
    .bind(&payload.on_busy)
    .bind(&payload.indicators)
    .bind(payload.is_active.unwrap_or(false))
    .fetch_one(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Parse indicators and cache the compiled strategy
    let indicators: Vec<crate::IndexId> = serde_json::from_value(payload.indicators.clone())
        .unwrap_or_default();
    {
        let mut cache = state.strategy_cache.write().await;
        cache.insert(row.id, CachedStrategy {
            compiled,
            indicators,
            name: payload.name.clone(),
        });
    }

    Ok((StatusCode::CREATED, Json(row)).into_response())
}

async fn update_strategy(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(payload): Json<SaveStrategyPayload>,
) -> Result<impl IntoResponse, StatusCode> {
    let id: sqlx::types::Uuid = id.parse().map_err(|_| StatusCode::BAD_REQUEST)?;

    // Validate scripts compile before persisting
    let compiled = match super::scripting::compile_strategy(
        &state.rhai_engine,
        &payload.on_idle,
        &payload.on_open,
        &payload.on_busy,
    ) {
        Ok(c) => c,
        Err(msg) => {
            return Ok((StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": msg }))).into_response());
        }
    };

    let row = sqlx::query_as::<_, super::db::StrategyRow>(
        "UPDATE strategies SET name = $1, on_idle = $2, on_open = $3, on_busy = $4, indicators = $5, is_active = $6, updated_at = now()
         WHERE id = $7 AND pubkey = $8
         RETURNING *",
    )
    .bind(&payload.name)
    .bind(&payload.on_idle)
    .bind(&payload.on_open)
    .bind(&payload.on_busy)
    .bind(&payload.indicators)
    .bind(payload.is_active.unwrap_or(false))
    .bind(id)
    .bind(&auth.pubkey)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match row {
        Some(r) => {
            // Update cache with recompiled strategy
            let indicators: Vec<crate::IndexId> = serde_json::from_value(payload.indicators.clone())
                .unwrap_or_default();
            {
                let mut cache = state.strategy_cache.write().await;
                cache.insert(id, CachedStrategy {
                    compiled,
                    indicators,
                    name: payload.name.clone(),
                });
            }
            Ok(Json(r).into_response())
        }
        None => Ok(StatusCode::NOT_FOUND.into_response()),
    }
}

async fn delete_strategy(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let id: sqlx::types::Uuid = id.parse().map_err(|_| StatusCode::BAD_REQUEST)?;

    let result = sqlx::query("DELETE FROM strategies WHERE id = $1 AND pubkey = $2")
        .bind(id)
        .bind(&auth.pubkey)
        .execute(&state.pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if result.rows_affected() == 0 {
        Ok(StatusCode::NOT_FOUND)
    } else {
        // Evict from cache
        {
            let mut cache = state.strategy_cache.write().await;
            cache.remove(&id);
        }
        Ok(StatusCode::NO_CONTENT)
    }
}

// ── API Key Route ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SetApiKeyPayload {
    api_key: String,
}

async fn set_api_key(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(payload): Json<SetApiKeyPayload>,
) -> impl IntoResponse {
    let encrypted = super::crypto::encrypt(&state.encryption_key, payload.api_key.as_bytes());

    sqlx::query("UPDATE users SET api_key_enc = $1 WHERE pubkey = $2")
        .bind(&encrypted)
        .bind(&auth.pubkey)
        .execute(&state.pool)
        .await
        .map(|_| StatusCode::OK)
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

// ── WebSocket Handler ────────────────────────────────────────────────────────

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(socket, state, auth.pubkey))
}

async fn handle_ws(socket: WebSocket, state: Arc<AppState>, pubkey: String) {
    let (mut ws_sender, mut ws_receiver) = socket.split();

    // Create channel for this connection
    let (tx, mut rx) = tokio::sync::mpsc::channel::<UpdateFrontend>(128);

    // Register in connections map
    {
        let mut conns = state.ws_connections.write().await;
        conns.entry(pubkey.clone()).or_default().push(tx);
    }

    info!("WebSocket connected for user {}", pubkey);

    // Send task: channel → WebSocket (outbound)
    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if let Ok(text) = serde_json::to_string(&msg) {
                if ws_sender.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
        }
    });

    // Receive task: WebSocket → handle (inbound)
    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_receiver.next().await {
            match msg {
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    // Wait for either task to finish
    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }

    // Cleanup: remove this sender from connections map
    {
        let mut conns = state.ws_connections.write().await;
        if let Some(senders) = conns.get_mut(&pubkey) {
            senders.retain(|s| !s.is_closed());
            if senders.is_empty() {
                conns.remove(&pubkey);
            }
        }
    }

    info!("WebSocket disconnected for user {}", pubkey);
}
