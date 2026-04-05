use std::sync::Arc;
use std::time::Instant;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Path, Query, State, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use log::info;
use serde::{Deserialize, Serialize};
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use super::app_state::{AppState, CachedStrategy, broadcast_to_user};
use super::auth::{self, AuthUser};
use super::db::{BacktestResultRow, BacktestRunRow};
use crate::backtest::{BacktestResult, BacktestRunRequest};
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
        // Backtest history
        .route("/backtest/history", get(list_backtest_history))
        .route(
            "/backtest/history/{id}",
            get(get_backtest_result).delete(delete_backtest_run),
        )
        // Data queries (authenticated)
        .route("/trades/{market}", get(get_trades))
        .route("/strategies", get(list_strategies).post(save_strategy))
        .route(
            "/strategies/{id}",
            get(get_strategy)
                .put(update_strategy)
                .delete(delete_strategy),
        )
        // Agent approval
        .route("/agent/prepare", post(prepare_agent))
        .route("/agent/approve", post(approve_agent_route))
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
    let token = auth::issue_jwt(&address, &state.jwt_secret)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(VerifyResponse { token }))
}

// ── Command Route ────────────────────────────────────────────────────────────

async fn execute_command(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(event): Json<BotEvent>,
) -> impl IntoResponse {
    let mut manager = state.bot_manager.write().await;
    let cmd_tx = match manager
        .get_or_create_bot(
            &auth.pubkey,
            &state.pool,
            &state.encryption_key,
            state.ws_connections.clone(),
            state.rhai_engine.clone(),
            state.strategy_cache.clone(),
        )
        .await
    {
        Ok(tx) => tx,
        Err(e) => {
            log::warn!("Bot creation failed for {}: {:?}", auth.pubkey, e);
            return (StatusCode::PRECONDITION_FAILED, e.to_string()).into_response();
        }
    };

    match cmd_tx.try_send(event) {
        Ok(()) => StatusCode::OK.into_response(),
        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
            StatusCode::TOO_MANY_REQUESTS.into_response()
        }
        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

// ── Backtest Route ───────────────────────────────────────────────────────────

#[inline]
fn make_backtest_run_id(asset: &str) -> String {
    format!("bt-{asset}-{}", get_time_now())
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
        return Json(
            serde_json::to_value(BacktestRunError {
                run_id,
                message,
                progress: Vec::new(),
            })
            .unwrap(),
        )
        .into_response();
    }

    request.run_id = Some(run_id.clone());

    // Per-user concurrency guard: only 1 active backtest per user
    {
        let active = state.active_backtests.read().await;
        if active.contains(&auth.pubkey) {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(
                    serde_json::to_value(BacktestRunError {
                        run_id,
                        message: "A backtest is already running".to_string(),
                        progress: Vec::new(),
                    })
                    .unwrap(),
                ),
            )
                .into_response();
        }
    }
    {
        let mut active = state.active_backtests.write().await;
        active.insert(auth.pubkey.clone());
    }

    let mut backtester = match Backtester::from_request(
        request,
        state.rhai_engine.clone(),
        state.strategy_cache.clone(),
        &state.pool,
        state.candle_store.clone(),
    )
    .await
    {
        Ok(bt) => bt,
        Err(e) => {
            // Remove from active set on failure
            let mut active = state.active_backtests.write().await;
            active.remove(&auth.pubkey);
            return Json(
                serde_json::to_value(BacktestRunError {
                    run_id,
                    message: e.to_string(),
                    progress: Vec::new(),
                })
                .unwrap(),
            )
            .into_response();
        }
    };
    let mut progress = Vec::new();

    let ws_conns = state.ws_connections.clone();
    let pubkey = auth.pubkey.clone();
    let progress_run_id = run_id.clone();

    let response = match backtester
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

            // Save to DB (fire-and-forget)
            let pool = state.pool.clone();
            let pk_save = auth.pubkey.clone();
            let strat_cache = state.strategy_cache.clone();
            let result_for_db = result.clone();
            tokio::spawn(async move {
                if let Err(e) =
                    save_backtest_to_db(&pool, &pk_save, &strat_cache, &result_for_db).await
                {
                    log::warn!("failed to save backtest to DB: {e}");
                }
            });

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
    };

    // Always remove from active set when done
    {
        let mut active = state.active_backtests.write().await;
        active.remove(&auth.pubkey);
    }

    response
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
    let rows = sqlx::query_as::<_, super::db::StrategySummary>(
        "SELECT id, name, is_active FROM strategies WHERE pubkey = $1 ORDER BY updated_at DESC",
    )
    .bind(&auth.pubkey)
    .fetch_all(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(rows))
}

async fn get_strategy(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<sqlx::types::Uuid>,
) -> Result<impl IntoResponse, StatusCode> {
    let row = sqlx::query_as::<_, super::db::StrategyRow>(
        "SELECT * FROM strategies WHERE id = $1 AND pubkey = $2",
    )
    .bind(id)
    .bind(&auth.pubkey)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;

    Ok(Json(row))
}

#[derive(Deserialize)]
struct SaveStrategyPayload {
    name: String,
    on_idle: String,
    on_open: String,
    on_busy: String,
    indicators: serde_json::Value,
    state_declarations: Option<serde_json::Value>,
    is_active: Option<bool>,
}

async fn save_strategy(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(payload): Json<SaveStrategyPayload>,
) -> Result<impl IntoResponse, StatusCode> {
    // Parse state declarations
    let state_decls: Option<super::scripting::StateDeclarations> = payload
        .state_declarations
        .as_ref()
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    // Validate scripts compile before persisting (expansion happens inside)
    let compiled = match super::scripting::compile_strategy(
        &state.rhai_engine,
        &payload.on_idle,
        &payload.on_open,
        &payload.on_busy,
        state_decls.as_ref(),
    ) {
        Ok(c) => c,
        Err(msg) => {
            return Ok((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": msg })),
            )
                .into_response());
        }
    };

    let row = sqlx::query_as::<_, super::db::StrategyRow>(
        "INSERT INTO strategies (pubkey, name, on_idle, on_open, on_busy, indicators, state_declarations, is_active)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         RETURNING *",
    )
    .bind(&auth.pubkey)
    .bind(&payload.name)
    .bind(&payload.on_idle)
    .bind(&payload.on_open)
    .bind(&payload.on_busy)
    .bind(&payload.indicators)
    .bind(&payload.state_declarations)
    .bind(payload.is_active.unwrap_or(false))
    .fetch_one(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Parse indicators and cache the compiled strategy
    let indicators: Vec<crate::IndexId> =
        serde_json::from_value(payload.indicators.clone()).unwrap_or_default();
    {
        let mut cache = state.strategy_cache.write().await;
        cache.insert(
            row.id,
            CachedStrategy {
                compiled,
                indicators,
                state_declarations: state_decls,
                name: payload.name.clone(),
            },
        );
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

    // Parse state declarations
    let state_decls: Option<super::scripting::StateDeclarations> = payload
        .state_declarations
        .as_ref()
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    // Validate scripts compile before persisting (expansion happens inside)
    let compiled = match super::scripting::compile_strategy(
        &state.rhai_engine,
        &payload.on_idle,
        &payload.on_open,
        &payload.on_busy,
        state_decls.as_ref(),
    ) {
        Ok(c) => c,
        Err(msg) => {
            return Ok((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": msg })),
            )
                .into_response());
        }
    };

    let row = sqlx::query_as::<_, super::db::StrategyRow>(
        "UPDATE strategies SET name = $1, on_idle = $2, on_open = $3, on_busy = $4, indicators = $5, state_declarations = $6, is_active = $7, updated_at = now()
         WHERE id = $8 AND pubkey = $9
         RETURNING *",
    )
    .bind(&payload.name)
    .bind(&payload.on_idle)
    .bind(&payload.on_open)
    .bind(&payload.on_busy)
    .bind(&payload.indicators)
    .bind(&payload.state_declarations)
    .bind(payload.is_active.unwrap_or(false))
    .bind(id)
    .bind(&auth.pubkey)
    .fetch_optional(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match row {
        Some(r) => {
            // Update cache with recompiled strategy
            let indicators: Vec<crate::IndexId> =
                serde_json::from_value(payload.indicators.clone()).unwrap_or_default();
            {
                let mut cache = state.strategy_cache.write().await;
                cache.insert(
                    id,
                    CachedStrategy {
                        compiled,
                        indicators,
                        state_declarations: state_decls,
                        name: payload.name.clone(),
                    },
                );
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

// ── Agent Approval Routes ────────────────────────────────────────────────────

async fn prepare_agent(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(payload): Json<PrepareAgentPayload>,
) -> impl IntoResponse {
    log::info!(
        "[agent/prepare] user={} requested agent preparation",
        auth.pubkey
    );

    let agent = alloy::signers::local::PrivateKeySigner::random();
    let nonce = auth::timestamp_nonce();

    let valid_until = get_time_now() + 180 * 86_400 * 1000; // 6 months in ms
    let prefix = payload
        .agent_name
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "kwant".to_string());
    let agent_name = format!("{prefix} valid_until {valid_until}");
    log::info!(
        "[agent/prepare] agent_name={agent_name}, agent_address={:?}, nonce={nonce}",
        agent.address()
    );

    let approve_agent = hyperliquid_rust_sdk::ApproveAgent {
        signature_chain_id: 1, // Ethereum mainnet for frontend signing
        hyperliquid_chain: "Mainnet".to_string(),
        agent_address: agent.address(),
        agent_name: Some(agent_name.clone()),
        nonce,
    };

    let eip712_payload = serde_json::json!({
        "domain": {
            "name": "HyperliquidSignTransaction",
            "version": "1",
            "chainId": 1,
            "verifyingContract": "0x0000000000000000000000000000000000000000"
        },
        "primaryType": "HyperliquidTransaction:ApproveAgent",
        "types": {
            "EIP712Domain": [
                {"name": "name", "type": "string"},
                {"name": "version", "type": "string"},
                {"name": "chainId", "type": "uint256"},
                {"name": "verifyingContract", "type": "address"}
            ],
            "HyperliquidTransaction:ApproveAgent": [
                {"name": "hyperliquidChain", "type": "string"},
                {"name": "agentAddress", "type": "address"},
                {"name": "agentName", "type": "string"},
                {"name": "nonce", "type": "uint64"}
            ]
        },
        "message": {
            "hyperliquidChain": "Mainnet",
            "signatureChainId": "0x1",
            "agentAddress": format!("{:?}", agent.address()),
            "agentName": agent_name,
            "nonce": nonce,
            "type": "approveAgent"
        }
    });

    {
        let mut store = state.pending_agents.write().await;
        store.insert(
            auth.pubkey,
            super::app_state::PendingAgent {
                agent_signer: agent,
                approve_agent,
                created_at: std::time::Instant::now(),
            },
        );
    }

    Json(serde_json::json!({ "eip712Payload": eip712_payload }))
}

#[derive(Deserialize)]
struct PrepareAgentPayload {
    agent_name: Option<String>,
}

#[derive(Deserialize)]
struct ApproveAgentPayload {
    signature: SignaturePayload,
}

#[derive(Deserialize, Serialize)]
struct SignaturePayload {
    r: String,
    s: String,
    v: u64,
}

async fn approve_agent_route(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(payload): Json<ApproveAgentPayload>,
) -> impl IntoResponse {
    log::info!(
        "[agent/approve] user={} submitting agent approval",
        auth.pubkey
    );

    // 1. Pop pending agent
    let pending = {
        let mut store = state.pending_agents.write().await;
        store.remove(&auth.pubkey)
    };
    let Some(pending) = pending else {
        log::warn!("[agent/approve] no pending agent for user={}", auth.pubkey);
        return (
            StatusCode::NOT_FOUND,
            "No pending agent — call /agent/prepare first",
        )
            .into_response();
    };
    if pending.created_at.elapsed().as_secs() > 300 {
        log::warn!(
            "[agent/approve] pending agent expired for user={}",
            auth.pubkey
        );
        return (
            StatusCode::GONE,
            "Pending agent expired — call /agent/prepare again",
        )
            .into_response();
    }

    // 2. Serialize action via SDK's Actions enum
    let action = match serde_json::to_value(hyperliquid_rust_sdk::Actions::ApproveAgent(
        pending.approve_agent.clone(),
    )) {
        Ok(v) => v,
        Err(e) => {
            log::error!("Failed to serialize ApproveAgent: {}", e);
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    // 3. Build exchange payload
    let exchange_payload = serde_json::json!({
        "action": action,
        "nonce": pending.approve_agent.nonce,
        "signature": {
            "r": payload.signature.r,
            "s": payload.signature.s,
            "v": payload.signature.v
        },
        "expiresAfter": null,
        "isFrontend": true,
        "vaultAddress": null
    });

    // 4. POST to Hyperliquid /exchange
    let body = serde_json::to_string(&exchange_payload).unwrap();
    log::debug!("[agent/approve] exchange payload: {body}");
    let client = reqwest::Client::new();
    let resp = match client
        .post("https://api.hyperliquid.xyz/exchange")
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            log::error!("Hyperliquid exchange request failed: {}", e);
            return (StatusCode::BAD_GATEWAY, "Failed to reach Hyperliquid").into_response();
        }
    };

    let hl_status = resp.status();
    let hl_body = resp.text().await.unwrap_or_default();
    log::info!("[agent/approve] HL /exchange responded: status={hl_status}, body={hl_body}");

    if !hl_status.is_success() {
        log::error!("[agent/approve] Hyperliquid request failed: {hl_body}");
        return (
            StatusCode::BAD_GATEWAY,
            format!("Hyperliquid rejected: {hl_body}"),
        )
            .into_response();
    }

    // HL returns 200 even on errors — check the JSON status field
    if let Ok(hl_json) = serde_json::from_str::<serde_json::Value>(&hl_body)
        && hl_json.get("status").and_then(|s| s.as_str()) == Some("err")
    {
        let msg = hl_json
            .get("response")
            .and_then(|r| r.as_str())
            .unwrap_or("Unknown error");
        log::error!("[agent/approve] Hyperliquid rejected agent approval: {msg}");
        return (
            StatusCode::BAD_GATEWAY,
            format!("Hyperliquid rejected: {msg}"),
        )
            .into_response();
    }

    // 5. Encrypt and store agent private key
    let agent_key_hex = hex::encode(pending.agent_signer.to_bytes());
    let encrypted = super::crypto::encrypt(&state.encryption_key, agent_key_hex.as_bytes());

    // valid_until is encoded in agent_name: "{prefix} valid_until {ms_timestamp}"
    let valid_until: i64 = pending
        .approve_agent
        .agent_name
        .as_deref()
        .and_then(|n| n.rsplit(' ').next())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    if let Err(e) =
        sqlx::query("UPDATE users SET api_key_enc = $1, agent_valid_until = $2 WHERE pubkey = $3")
            .bind(&encrypted)
            .bind(valid_until)
            .bind(&auth.pubkey)
            .execute(&state.pool)
            .await
    {
        log::error!("Failed to store agent key for {}: {}", auth.pubkey, e);
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }

    log::info!(
        "[agent/approve] agent key stored for user={}, valid_until={}",
        auth.pubkey,
        valid_until
    );

    // 6. Hot-reload existing bot or spawn a new one
    {
        let manager = state.bot_manager.read().await;
        if manager
            .reload_wallet(&auth.pubkey, pending.agent_signer)
            .await
        {
            log::info!("[agent/approve] hot-reloaded wallet for existing bot");
        } else {
            drop(manager);
            let mut manager = state.bot_manager.write().await;
            if let Err(e) = manager
                .get_or_create_bot(
                    &auth.pubkey,
                    &state.pool,
                    &state.encryption_key,
                    state.ws_connections.clone(),
                    state.rhai_engine.clone(),
                    state.strategy_cache.clone(),
                )
                .await
            {
                log::warn!(
                    "Bot creation after agent approval failed for {}: {:?}",
                    auth.pubkey,
                    e
                );
            }
        }
    }

    log::info!(
        "[agent/approve] agent approval complete for user={}",
        auth.pubkey
    );
    StatusCode::OK.into_response()
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
            if let Ok(text) = serde_json::to_string(&msg)
                && ws_sender.send(Message::Text(text.into())).await.is_err()
            {
                break;
            }
        }
    });

    // Receive task: WebSocket → handle (inbound)
    let recv_task = tokio::spawn(async move {
        #[allow(clippy::never_loop)]
        while let Some(Ok(msg)) = ws_receiver.next().await
            && let Message::Close(_) = msg
        {
            break;
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

// ── Backtest Persistence ────────────────────────────────────────────────────

async fn save_backtest_to_db(
    pool: &sqlx::PgPool,
    pubkey: &str,
    strategy_cache: &super::app_state::StrategyCache,
    result: &BacktestResult,
) -> Result<(), String> {
    let cfg = &result.config;
    let s = &result.summary;

    // Resolve strategy name from cache or DB
    let strategy_name = {
        let guard = strategy_cache.read().await;
        guard.get(&cfg.strategy_id).map(|c| c.name.clone())
    };
    let strategy_name = match strategy_name {
        Some(name) => name,
        None => sqlx::query_scalar::<_, String>("SELECT name FROM strategies WHERE id = $1")
            .bind(cfg.strategy_id)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| "Unknown".to_string()),
    };

    let exchange_str = cfg.source.exchange.name();
    let market_str = cfg.source.market.as_str();

    // Insert into backtest_runs
    let run_row_id = sqlx::query_scalar::<_, sqlx::types::Uuid>(
        "INSERT INTO backtest_runs (
            pubkey, strategy_id, strategy_name, asset, resolution,
            exchange, market, margin, lev, start_time, end_time,
            net_pnl, return_pct, max_drawdown_pct, total_trades,
            win_rate_pct, profit_factor, sharpe_ratio,
            started_at, finished_at
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11,
            $12, $13, $14, $15, $16, $17, $18, $19, $20
        ) RETURNING id",
    )
    .bind(pubkey)
    .bind(cfg.strategy_id)
    .bind(&strategy_name)
    .bind(&cfg.asset)
    .bind(cfg.resolution.to_string())
    .bind(exchange_str)
    .bind(market_str)
    .bind(cfg.margin)
    .bind(cfg.lev as i32)
    .bind(cfg.start_time as i64)
    .bind(cfg.end_time as i64)
    .bind(s.net_pnl)
    .bind(s.return_pct)
    .bind(s.max_drawdown_pct)
    .bind(s.total_trades as i32)
    .bind(s.win_rate_pct)
    .bind(s.profit_factor)
    .bind(s.sharpe_ratio)
    .bind(result.started_at as i64)
    .bind(result.finished_at as i64)
    .fetch_one(pool)
    .await
    .map_err(|e| format!("insert backtest_runs: {e}"))?;

    // Insert into backtest_results
    let trades_json =
        serde_json::to_value(&result.trades).map_err(|e| format!("serialize trades: {e}"))?;
    let equity_json =
        serde_json::to_value(&result.equity_curve).map_err(|e| format!("serialize equity: {e}"))?;
    let snapshots_json =
        serde_json::to_value(&result.snapshots).map_err(|e| format!("serialize snapshots: {e}"))?;

    sqlx::query(
        "INSERT INTO backtest_results (
            run_id, initial_equity, final_equity,
            gross_profit, gross_loss, avg_win, avg_loss, expectancy,
            wins, losses, candles_loaded, candles_processed,
            max_drawdown_abs, trades, equity_curve, snapshots
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16
        )",
    )
    .bind(run_row_id)
    .bind(s.initial_equity)
    .bind(s.final_equity)
    .bind(s.gross_profit)
    .bind(s.gross_loss)
    .bind(s.avg_win)
    .bind(s.avg_loss)
    .bind(s.expectancy)
    .bind(s.wins as i32)
    .bind(s.losses as i32)
    .bind(result.candles_loaded as i64)
    .bind(result.candles_processed as i64)
    .bind(s.max_drawdown_abs)
    .bind(&trades_json)
    .bind(&equity_json)
    .bind(&snapshots_json)
    .execute(pool)
    .await
    .map_err(|e| format!("insert backtest_results: {e}"))?;

    info!(
        "backtest saved: run={} asset={} strategy={}",
        run_row_id, cfg.asset, strategy_name
    );
    Ok(())
}

// ── Backtest History Routes ─────────────────────────────────────────────────

#[derive(Deserialize)]
struct BacktestHistoryQuery {
    asset: Option<String>,
    strategy_id: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

async fn list_backtest_history(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(params): Query<BacktestHistoryQuery>,
) -> Result<impl IntoResponse, StatusCode> {
    let limit = params.limit.unwrap_or(50).min(200);
    let offset = params.offset.unwrap_or(0);

    let rows = match (&params.asset, &params.strategy_id) {
        (Some(asset), Some(sid)) => {
            let strategy_id: uuid::Uuid = sid.parse().map_err(|_| StatusCode::BAD_REQUEST)?;
            sqlx::query_as::<_, BacktestRunRow>(
                "SELECT * FROM backtest_runs
                 WHERE pubkey = $1 AND asset = $2 AND strategy_id = $3
                 ORDER BY created_at DESC LIMIT $4 OFFSET $5",
            )
            .bind(&auth.pubkey)
            .bind(asset)
            .bind(strategy_id)
            .bind(limit)
            .bind(offset)
            .fetch_all(&state.pool)
            .await
        }
        (Some(asset), None) => {
            sqlx::query_as::<_, BacktestRunRow>(
                "SELECT * FROM backtest_runs
                 WHERE pubkey = $1 AND asset = $2
                 ORDER BY created_at DESC LIMIT $3 OFFSET $4",
            )
            .bind(&auth.pubkey)
            .bind(asset)
            .bind(limit)
            .bind(offset)
            .fetch_all(&state.pool)
            .await
        }
        (None, Some(sid)) => {
            let strategy_id: uuid::Uuid = sid.parse().map_err(|_| StatusCode::BAD_REQUEST)?;
            sqlx::query_as::<_, BacktestRunRow>(
                "SELECT * FROM backtest_runs
                 WHERE pubkey = $1 AND strategy_id = $2
                 ORDER BY created_at DESC LIMIT $3 OFFSET $4",
            )
            .bind(&auth.pubkey)
            .bind(strategy_id)
            .bind(limit)
            .bind(offset)
            .fetch_all(&state.pool)
            .await
        }
        (None, None) => {
            sqlx::query_as::<_, BacktestRunRow>(
                "SELECT * FROM backtest_runs
                 WHERE pubkey = $1
                 ORDER BY created_at DESC LIMIT $2 OFFSET $3",
            )
            .bind(&auth.pubkey)
            .bind(limit)
            .bind(offset)
            .fetch_all(&state.pool)
            .await
        }
    };

    match rows {
        Ok(rows) => Ok(Json(serde_json::to_value(rows).unwrap())),
        Err(e) => {
            log::warn!("list_backtest_history error: {e}");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn get_backtest_result(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<impl IntoResponse, StatusCode> {
    // Verify ownership
    let owns = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM backtest_runs WHERE id = $1 AND pubkey = $2)",
    )
    .bind(id)
    .bind(&auth.pubkey)
    .fetch_one(&state.pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if !owns {
        return Err(StatusCode::NOT_FOUND);
    }

    let row =
        sqlx::query_as::<_, BacktestResultRow>("SELECT * FROM backtest_results WHERE run_id = $1")
            .bind(id)
            .fetch_optional(&state.pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match row {
        Some(r) => Ok(Json(serde_json::to_value(r).unwrap())),
        None => Err(StatusCode::NOT_FOUND),
    }
}

async fn delete_backtest_run(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<uuid::Uuid>,
) -> impl IntoResponse {
    let result = sqlx::query("DELETE FROM backtest_runs WHERE id = $1 AND pubkey = $2")
        .bind(id)
        .bind(&auth.pubkey)
        .execute(&state.pool)
        .await;

    match result {
        Ok(r) if r.rows_affected() > 0 => StatusCode::NO_CONTENT,
        Ok(_) => StatusCode::NOT_FOUND,
        Err(e) => {
            log::warn!("delete_backtest_run error: {e}");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}
