use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use alloy::primitives::Address;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Path, Query, State, WebSocketUpgrade};
use axum::http::{HeaderValue, Method, Request, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::StreamExt;
use log::info;
use serde::{Deserialize, Serialize};
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use super::app_state::{AppState, CachedStrategy, WsConnection, broadcast_to_user};
use super::auth::{self, AuthUser};
use super::db::{BacktestResultRow, BacktestRunRow};
use crate::backtest::{BacktestResult, BacktestRunRequest};
use crate::metrics::{RuntimeMetricsSnapshot, runtime_metrics_snapshot};
use crate::{
    BacktestProgressUpdate, BacktestResultUpdate, BacktestRunError, BacktestRunPayload,
    BacktestRunResponse, Backtester, Bot, BotEvent, UpdateFrontend, get_time_now,
};

const WS_SEND_TIMEOUT_SECS: u64 = 5;
const DEFAULT_PAGE_LIMIT: i64 = 50;
const MAX_PAGE_LIMIT: i64 = 200;
const DEFAULT_STRATEGY_LIST_LIMIT: i64 = 200;
const MAX_STRATEGY_LIST_LIMIT: i64 = 500;
const HYPERLIQUID_HTTP_TIMEOUT_SECS: u64 = 10;
const AGENT_NAME_PREFIX_MAX_LEN: usize = 64;
const STRATEGY_NAME_MAX_LEN: usize = 128;
const STRATEGY_SCRIPT_MAX_BYTES: usize = 64 * 1024;
const STRATEGY_INDICATORS_MAX: usize = 512;
const MARKET_PATH_MAX_LEN: usize = 64;
const NONCE_TTL_SECS: u64 = 300;
const MAX_PENDING_NONCES: usize = 10_000;
const READINESS_DB_TIMEOUT_SECS: u64 = 2;
const DB_QUERY_TIMEOUT_SECS: u64 = 10;
const BOT_STARTUP_WAIT_TIMEOUT_SECS: u64 = 90;
const BOT_STARTUP_WAIT_POLL_MS: u64 = 50;

pub fn create_router(state: Arc<AppState>) -> Router {
    Router::new()
        // Health (unauthenticated)
        .route("/healthz", get(healthz))
        .route("/readyz", get(readyz))
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
        .route("/metrics", get(get_metrics))
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
        .layer(cors_layer_from_env())
        .layer(
            TraceLayer::new_for_http().make_span_with(|request: &Request<_>| {
                tracing::info_span!(
                    "http_request",
                    method = %request.method(),
                    path = %request.uri().path()
                )
            }),
        )
        .with_state(state)
}

// ── Auth Routes ──────────────────────────────────────────────────────────────

async fn healthz() -> StatusCode {
    StatusCode::NO_CONTENT
}

async fn readyz(State(state): State<Arc<AppState>>) -> StatusCode {
    match tokio::time::timeout(
        Duration::from_secs(READINESS_DB_TIMEOUT_SECS),
        sqlx::query_scalar::<_, i64>("SELECT 1").fetch_one(&state.pool),
    )
    .await
    {
        Ok(Ok(1)) => StatusCode::NO_CONTENT,
        Ok(Ok(_)) | Ok(Err(_)) | Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}

async fn get_metrics(_auth: AuthUser) -> Json<RuntimeMetricsSnapshot> {
    Json(runtime_metrics_snapshot())
}

async fn db_query_timeout<T, E, F>(label: &str, fut: F) -> Result<T, StatusCode>
where
    F: Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    match tokio::time::timeout(Duration::from_secs(DB_QUERY_TIMEOUT_SECS), fut).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(err)) => {
            log::warn!("database query {label} failed: {err}");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
        Err(_) => {
            log::warn!("database query {label} timed out");
            Err(StatusCode::SERVICE_UNAVAILABLE)
        }
    }
}

async fn db_query_timeout_string<T, E, F>(label: &str, fut: F) -> Result<T, String>
where
    F: Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    match tokio::time::timeout(Duration::from_secs(DB_QUERY_TIMEOUT_SECS), fut).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(err)) => Err(format!("database query {label} failed: {err}")),
        Err(_) => Err(format!("database query {label} timed out")),
    }
}

enum CorsPolicy {
    Permissive,
    Origins(Vec<HeaderValue>),
}

fn cors_layer_from_env() -> CorsLayer {
    match std::env::var("CORS_ORIGINS") {
        Ok(raw) => match parse_cors_policy(&raw) {
            Ok(CorsPolicy::Permissive) => CorsLayer::permissive(),
            Ok(CorsPolicy::Origins(origins)) => CorsLayer::new()
                .allow_origin(origins)
                .allow_methods([
                    Method::GET,
                    Method::POST,
                    Method::PUT,
                    Method::PATCH,
                    Method::DELETE,
                ])
                .allow_headers(Any),
            Err(err) => {
                log::error!(
                    "Invalid CORS_ORIGINS; browser cross-origin requests will be denied: {err}"
                );
                fail_closed_cors_layer()
            }
        },
        Err(_) => {
            log::warn!("CORS_ORIGINS not set; browser cross-origin requests will be denied");
            fail_closed_cors_layer()
        }
    }
}

fn fail_closed_cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
        ])
        .allow_headers(Any)
}

fn parse_cors_policy(raw: &str) -> Result<CorsPolicy, String> {
    if raw.trim() == "*" {
        return Ok(CorsPolicy::Permissive);
    }

    parse_cors_origins(raw).map(CorsPolicy::Origins)
}

fn parse_cors_origins(raw: &str) -> Result<Vec<HeaderValue>, String> {
    let origins = raw
        .split(',')
        .map(str::trim)
        .filter(|origin| !origin.is_empty())
        .map(|origin| {
            origin
                .parse::<HeaderValue>()
                .map_err(|err| format!("{origin:?}: {err}"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    if origins.is_empty() {
        return Err("no origins configured".to_string());
    }

    Ok(origins)
}

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
) -> Result<Json<NonceResponse>, StatusCode> {
    let address = normalize_auth_address(&params.address)?;

    let nonce = {
        let mut store = state.nonces.write().await;
        issue_nonce_for_address(&mut store, address, Instant::now())?
    };

    Ok(Json(NonceResponse { nonce }))
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
    let address = normalize_auth_address(&payload.address)?;

    {
        let mut store = state.nonces.write().await;
        verify_nonce_signature_and_consume(
            &mut store,
            &address,
            &payload.signature,
            &payload.nonce,
            Instant::now(),
        )?;
    }

    // Upsert user in DB
    db_query_timeout(
        "verify_signature upsert user",
        sqlx::query("INSERT INTO users (pubkey) VALUES ($1) ON CONFLICT (pubkey) DO NOTHING")
            .bind(&address)
            .execute(&state.pool),
    )
    .await?;

    // Issue JWT
    let token = auth::issue_jwt(&address, &state.jwt_secret)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(VerifyResponse { token }))
}

fn normalize_auth_address(address: &str) -> Result<String, StatusCode> {
    let address = address
        .trim()
        .parse::<Address>()
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    Ok(format!("{address:#x}").to_ascii_lowercase())
}

fn prune_expired_nonces(
    store: &mut std::collections::HashMap<String, (String, Instant)>,
    now: Instant,
) {
    store.retain(|_, (_, created_at)| !nonce_is_expired(*created_at, now));
}

fn issue_nonce_for_address(
    store: &mut std::collections::HashMap<String, (String, Instant)>,
    address: String,
    now: Instant,
) -> Result<String, StatusCode> {
    prune_expired_nonces(store, now);
    if let Some((existing_nonce, _)) = store.get(&address) {
        return Ok(existing_nonce.clone());
    }
    if store.len() >= MAX_PENDING_NONCES {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }

    let nonce = auth::generate_nonce();
    store.insert(address, (nonce.clone(), now));
    Ok(nonce)
}

fn verify_nonce_signature_and_consume(
    store: &mut std::collections::HashMap<String, (String, Instant)>,
    address: &str,
    signature: &str,
    nonce: &str,
    now: Instant,
) -> Result<(), StatusCode> {
    let (expected_nonce, created_at) =
        store.get(address).cloned().ok_or(StatusCode::BAD_REQUEST)?;

    if nonce_is_expired(created_at, now) {
        store.remove(address);
        return Err(StatusCode::GONE);
    }

    if nonce != expected_nonce {
        return Err(StatusCode::BAD_REQUEST);
    }

    auth::verify_signature(address, signature, nonce).map_err(|_| StatusCode::UNAUTHORIZED)?;
    store.remove(address);
    Ok(())
}

fn nonce_is_expired(created_at: Instant, now: Instant) -> bool {
    now.saturating_duration_since(created_at).as_secs() > NONCE_TTL_SECS
}

// ── Command Route ────────────────────────────────────────────────────────────

async fn execute_command(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(event): Json<BotEvent>,
) -> impl IntoResponse {
    let cmd_tx = match get_or_create_bot_sender(&state, &auth.pubkey).await {
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

async fn live_bot_sender(
    state: &Arc<AppState>,
    pubkey: &str,
) -> Option<tokio::sync::mpsc::Sender<BotEvent>> {
    let manager = state.bot_manager.read().await;
    manager.get_bot(pubkey).filter(|tx| !tx.is_closed())
}

async fn get_or_create_bot_sender(
    state: &Arc<AppState>,
    pubkey: &str,
) -> Result<tokio::sync::mpsc::Sender<BotEvent>, crate::Error> {
    let deadline = Instant::now() + Duration::from_secs(BOT_STARTUP_WAIT_TIMEOUT_SECS);

    loop {
        if let Some(tx) = live_bot_sender(state, pubkey).await {
            return Ok(tx);
        }

        if let Some(_startup_guard) =
            BotStartupGuard::acquire(Arc::clone(&state.bot_startups), pubkey.to_string()).await
        {
            if let Some(tx) = live_bot_sender(state, pubkey).await {
                return Ok(tx);
            }

            let build_context = {
                let manager = state.bot_manager.read().await;
                manager.build_context()
            };

            let (bot, cmd_tx) = build_context
                .build_bot(pubkey, &state.pool, &state.encryption_key)
                .await?;

            let registered_tx = {
                let mut manager = state.bot_manager.write().await;
                manager.register_bot_if_absent(pubkey.to_string(), cmd_tx.clone())
            };

            if registered_tx.same_channel(&cmd_tx) {
                let task = spawn_bot(bot, pubkey.to_string(), state.clone(), cmd_tx.clone());
                let mut manager = state.bot_manager.write().await;
                manager.attach_bot_task(pubkey, &cmd_tx, task);
            } else {
                bot.shutdown_unused().await;
            }

            return Ok(registered_tx);
        }

        if Instant::now() >= deadline {
            return Err(crate::Error::Custom(format!(
                "timed out waiting for bot startup for {pubkey}"
            )));
        }

        tokio::time::sleep(Duration::from_millis(BOT_STARTUP_WAIT_POLL_MS)).await;
    }
}

fn spawn_bot(
    bot: Bot,
    pubkey: String,
    state: Arc<AppState>,
    cmd_tx: tokio::sync::mpsc::Sender<BotEvent>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let cleanup_pubkey = pubkey.clone();
        let ws_connections = state.ws_connections.clone();
        let pool = state.pool.clone();
        let rhai_engine = state.rhai_engine.clone();
        let strategy_cache = state.strategy_cache.clone();
        if let Err(e) = bot
            .start(ws_connections, pubkey, pool, rhai_engine, strategy_cache)
            .await
        {
            log::error!("Bot exited with error: {:?}", e);
        }

        let mut manager = state.bot_manager.write().await;
        manager.remove_if_sender(&cleanup_pubkey, &cmd_tx);
    })
}

struct BotStartupGuard {
    active: Arc<tokio::sync::RwLock<std::collections::HashSet<String>>>,
    pubkey: Option<String>,
}

impl BotStartupGuard {
    async fn acquire(
        active: Arc<tokio::sync::RwLock<std::collections::HashSet<String>>>,
        pubkey: String,
    ) -> Option<Self> {
        {
            let mut guard = active.write().await;
            if !guard.insert(pubkey.clone()) {
                return None;
            }
        }

        Some(Self {
            active,
            pubkey: Some(pubkey),
        })
    }
}

impl Drop for BotStartupGuard {
    fn drop(&mut self) {
        if let Some(pubkey) = self.pubkey.take() {
            let active = Arc::clone(&self.active);
            tokio::spawn(async move {
                active.write().await.remove(&pubkey);
            });
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
        return Json(BacktestRunError {
            run_id,
            message,
            progress: Vec::new(),
        })
        .into_response();
    }

    request.run_id = Some(run_id.clone());

    // Per-user concurrency guard: only 1 active backtest per user
    let active_guard = match ActiveBacktestGuard::acquire(
        Arc::clone(&state.active_backtests),
        auth.pubkey.clone(),
    )
    .await
    {
        Some(guard) => guard,
        None => {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(BacktestRunError {
                    run_id,
                    message: "A backtest is already running".to_string(),
                    progress: Vec::new(),
                }),
            )
                .into_response();
        }
    };

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
            active_guard.release().await;
            return Json(BacktestRunError {
                run_id,
                message: e.to_string(),
                progress: Vec::new(),
            })
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

            Json(BacktestRunResponse {
                run_id,
                result,
                progress,
            })
            .into_response()
        }
        Err(err) => Json(BacktestRunError {
            run_id,
            message: err.to_string(),
            progress,
        })
        .into_response(),
    };

    active_guard.release().await;

    response
}

struct ActiveBacktestGuard {
    active: Arc<tokio::sync::RwLock<std::collections::HashSet<String>>>,
    pubkey: Option<String>,
}

impl ActiveBacktestGuard {
    async fn acquire(
        active: Arc<tokio::sync::RwLock<std::collections::HashSet<String>>>,
        pubkey: String,
    ) -> Option<Self> {
        {
            let mut guard = active.write().await;
            if !guard.insert(pubkey.clone()) {
                return None;
            }
        }

        Some(Self {
            active,
            pubkey: Some(pubkey),
        })
    }

    async fn release(mut self) {
        if let Some(pubkey) = self.pubkey.take() {
            self.active.write().await.remove(&pubkey);
        }
    }
}

impl Drop for ActiveBacktestGuard {
    fn drop(&mut self) {
        if let Some(pubkey) = self.pubkey.take() {
            let active = Arc::clone(&self.active);
            tokio::spawn(async move {
                active.write().await.remove(&pubkey);
            });
        }
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
    validate_market_path(&market)?;
    let (limit, offset) = bounded_pagination(params.limit, params.offset);

    let rows = db_query_timeout(
        "get_trades",
        sqlx::query_as::<_, super::db::TradeRow>(
        "SELECT * FROM trades WHERE pubkey = $1 AND market = $2 ORDER BY close_time DESC LIMIT $3 OFFSET $4",
    )
    .bind(&auth.pubkey)
    .bind(&market)
    .bind(limit)
    .bind(offset)
            .fetch_all(&state.pool),
    )
    .await?;

    Ok(Json(rows))
}

// ── Strategies Routes ────────────────────────────────────────────────────────

async fn list_strategies(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Query(params): Query<StrategyListQueryParams>,
) -> Result<impl IntoResponse, StatusCode> {
    let (limit, offset) = bounded_strategy_list_pagination(params.limit, params.offset);
    let rows = db_query_timeout(
        "list_strategies",
        sqlx::query_as::<_, super::db::StrategySummary>(
        "SELECT id, name, is_active FROM strategies WHERE pubkey = $1 ORDER BY updated_at DESC LIMIT $2 OFFSET $3",
    )
    .bind(&auth.pubkey)
    .bind(limit)
    .bind(offset)
            .fetch_all(&state.pool),
    )
    .await?;

    Ok(Json(rows))
}

async fn get_strategy(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<sqlx::types::Uuid>,
) -> Result<impl IntoResponse, StatusCode> {
    let row = db_query_timeout(
        "get_strategy",
        sqlx::query_as::<_, super::db::StrategyRow>(
            "SELECT * FROM strategies WHERE id = $1 AND pubkey = $2",
        )
        .bind(id)
        .bind(&auth.pubkey)
        .fetch_optional(&state.pool),
    )
    .await?
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

#[derive(Deserialize)]
struct StrategyListQueryParams {
    limit: Option<i64>,
    offset: Option<i64>,
}

async fn save_strategy(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(payload): Json<SaveStrategyPayload>,
) -> Result<impl IntoResponse, StatusCode> {
    if let Some(response) = validate_strategy_payload_bounds(&payload) {
        return Ok(response);
    }
    let strategy_name = normalized_strategy_name(&payload);

    let state_decls: Option<super::scripting::StateDeclarations> =
        match payload.state_declarations.as_ref() {
            Some(value) => match serde_json::from_value(value.clone()) {
                Ok(decls) => Some(decls),
                Err(err) => {
                    return Ok((
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({
                            "error": format!("invalid state declarations: {err}")
                        })),
                    )
                        .into_response());
                }
            },
            None => None,
        };
    let indicators: Vec<crate::IndexId> = match serde_json::from_value(payload.indicators.clone()) {
        Ok(indicators) => indicators,
        Err(err) => {
            return Ok((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!("invalid indicators: {err}")
                })),
            )
                .into_response());
        }
    };

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

    let row = db_query_timeout(
        "save_strategy",
        sqlx::query_as::<_, super::db::StrategyRow>(
        "INSERT INTO strategies (pubkey, name, on_idle, on_open, on_busy, indicators, state_declarations, is_active)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
         RETURNING *",
    )
    .bind(&auth.pubkey)
    .bind(&strategy_name)
    .bind(&payload.on_idle)
    .bind(&payload.on_open)
    .bind(&payload.on_busy)
    .bind(&payload.indicators)
    .bind(&payload.state_declarations)
    .bind(payload.is_active.unwrap_or(false))
            .fetch_one(&state.pool),
    )
    .await?;

    {
        let mut cache = state.strategy_cache.write().await;
        cache.insert(
            row.id,
            CachedStrategy {
                compiled,
                indicators,
                state_declarations: state_decls,
                name: strategy_name,
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
    if let Some(response) = validate_strategy_payload_bounds(&payload) {
        return Ok(response);
    }
    let strategy_name = normalized_strategy_name(&payload);

    let state_decls: Option<super::scripting::StateDeclarations> =
        match payload.state_declarations.as_ref() {
            Some(value) => match serde_json::from_value(value.clone()) {
                Ok(decls) => Some(decls),
                Err(err) => {
                    return Ok((
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({
                            "error": format!("invalid state declarations: {err}")
                        })),
                    )
                        .into_response());
                }
            },
            None => None,
        };
    let indicators: Vec<crate::IndexId> = match serde_json::from_value(payload.indicators.clone()) {
        Ok(indicators) => indicators,
        Err(err) => {
            return Ok((
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!("invalid indicators: {err}")
                })),
            )
                .into_response());
        }
    };

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

    let row = db_query_timeout(
        "update_strategy",
        sqlx::query_as::<_, super::db::StrategyRow>(
        "UPDATE strategies SET name = $1, on_idle = $2, on_open = $3, on_busy = $4, indicators = $5, state_declarations = $6, is_active = $7, updated_at = now()
         WHERE id = $8 AND pubkey = $9
         RETURNING *",
    )
    .bind(&strategy_name)
    .bind(&payload.on_idle)
    .bind(&payload.on_open)
    .bind(&payload.on_busy)
    .bind(&payload.indicators)
    .bind(&payload.state_declarations)
    .bind(payload.is_active.unwrap_or(false))
    .bind(id)
    .bind(&auth.pubkey)
            .fetch_optional(&state.pool),
    )
    .await?;

    match row {
        Some(r) => {
            // Update cache with recompiled strategy
            {
                let mut cache = state.strategy_cache.write().await;
                cache.insert(
                    id,
                    CachedStrategy {
                        compiled,
                        indicators,
                        state_declarations: state_decls,
                        name: strategy_name,
                    },
                );
            }
            Ok(Json(r).into_response())
        }
        None => Ok(StatusCode::NOT_FOUND.into_response()),
    }
}

fn validate_strategy_payload_bounds(
    payload: &SaveStrategyPayload,
) -> Option<axum::response::Response> {
    let name = normalized_strategy_name(payload);
    if name.is_empty() {
        return Some(strategy_validation_error("strategy name is required"));
    }

    if name.chars().count() > STRATEGY_NAME_MAX_LEN {
        return Some(strategy_validation_error(format!(
            "strategy name must be at most {STRATEGY_NAME_MAX_LEN} characters"
        )));
    }

    for (label, script) in [
        ("on_idle", &payload.on_idle),
        ("on_open", &payload.on_open),
        ("on_busy", &payload.on_busy),
    ] {
        if script.len() > STRATEGY_SCRIPT_MAX_BYTES {
            return Some(strategy_validation_error(format!(
                "{label} must be at most {STRATEGY_SCRIPT_MAX_BYTES} bytes"
            )));
        }
    }

    if payload
        .indicators
        .as_array()
        .is_some_and(|indicators| indicators.len() > STRATEGY_INDICATORS_MAX)
    {
        return Some(strategy_validation_error(format!(
            "strategies may reference at most {STRATEGY_INDICATORS_MAX} indicators"
        )));
    }

    None
}

fn normalized_strategy_name(payload: &SaveStrategyPayload) -> String {
    payload.name.trim().to_string()
}

fn strategy_validation_error(message: impl Into<String>) -> axum::response::Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "error": message.into() })),
    )
        .into_response()
}

async fn delete_strategy(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let id: sqlx::types::Uuid = id.parse().map_err(|_| StatusCode::BAD_REQUEST)?;

    let result = db_query_timeout(
        "delete_strategy",
        sqlx::query("DELETE FROM strategies WHERE id = $1 AND pubkey = $2")
            .bind(id)
            .bind(&auth.pubkey)
            .execute(&state.pool),
    )
    .await?;

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
    let prefix = match normalize_agent_name_prefix(payload.agent_name.as_deref()) {
        Ok(prefix) => prefix,
        Err((status, message)) => return (status, message).into_response(),
    };
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

    Json(serde_json::json!({ "eip712Payload": eip712_payload })).into_response()
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
    let body = match serde_json::to_string(&exchange_payload) {
        Ok(body) => body,
        Err(err) => {
            log::error!("[agent/approve] failed to serialize exchange payload: {err}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to submit API key approval".to_string(),
            )
                .into_response();
        }
    };
    log::debug!("[agent/approve] exchange payload bytes={}", body.len());
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(HYPERLIQUID_HTTP_TIMEOUT_SECS))
        .build()
    {
        Ok(client) => client,
        Err(err) => {
            log::error!("[agent/approve] failed to build HTTP client: {err}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to submit API key approval".to_string(),
            )
                .into_response();
        }
    };
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
    let hl_body = match resp.text().await {
        Ok(body) => body,
        Err(err) => {
            log::error!("[agent/approve] failed to read Hyperliquid response body: {err}");
            return (
                StatusCode::BAD_GATEWAY,
                "Failed to read Hyperliquid response".to_string(),
            )
                .into_response();
        }
    };
    log::info!("[agent/approve] HL /exchange responded: status={hl_status}");

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
    let encrypted = match super::crypto::encrypt(&state.encryption_key, agent_key_hex.as_bytes()) {
        Ok(encrypted) => encrypted,
        Err(err) => {
            log::error!("[agent/approve] failed to encrypt agent key: {err}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to store API key".to_string(),
            )
                .into_response();
        }
    };

    // valid_until is encoded in agent_name: "{prefix} valid_until {ms_timestamp}"
    let valid_until: i64 = pending
        .approve_agent
        .agent_name
        .as_deref()
        .and_then(|n| n.rsplit(' ').next())
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    if let Err(e) = db_query_timeout(
        "approve_agent update user key",
        sqlx::query("UPDATE users SET api_key_enc = $1, agent_valid_until = $2 WHERE pubkey = $3")
            .bind(&encrypted)
            .bind(valid_until)
            .bind(&auth.pubkey)
            .execute(&state.pool),
    )
    .await
    {
        return e.into_response();
    }

    log::info!(
        "[agent/approve] agent key stored for user={}, valid_until={}",
        auth.pubkey,
        valid_until
    );

    // 6. Hot-reload existing bot or spawn a new one
    if let Some(tx) = live_bot_sender(&state, &auth.pubkey).await {
        if queue_reload_wallet(&auth.pubkey, tx, pending.agent_signer.clone()).await {
            log::info!("[agent/approve] hot-reloaded wallet for existing bot");
        } else {
            log::warn!(
                "[agent/approve] existing bot channel closed during wallet reload for {}",
                auth.pubkey
            );
        }
    } else if let Err(e) = get_or_create_bot_sender(&state, &auth.pubkey).await {
        log::warn!(
            "Bot creation after agent approval failed for {}: {:?}",
            auth.pubkey,
            e
        );
    }

    log::info!(
        "[agent/approve] agent approval complete for user={}",
        auth.pubkey
    );
    StatusCode::OK.into_response()
}

fn normalize_agent_name_prefix(input: Option<&str>) -> Result<String, (StatusCode, String)> {
    let prefix = input
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("kwant");

    if prefix.chars().count() > AGENT_NAME_PREFIX_MAX_LEN {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("agent_name must be at most {AGENT_NAME_PREFIX_MAX_LEN} characters"),
        ));
    }

    if prefix.chars().any(char::is_control) {
        return Err((
            StatusCode::BAD_REQUEST,
            "agent_name must not contain control characters".to_string(),
        ));
    }

    Ok(prefix.to_string())
}

async fn queue_reload_wallet(
    pubkey: &str,
    tx: tokio::sync::mpsc::Sender<BotEvent>,
    signer: alloy::signers::local::PrivateKeySigner,
) -> bool {
    match tx.try_send(BotEvent::ReloadWallet(signer)) {
        Ok(()) => true,
        Err(tokio::sync::mpsc::error::TrySendError::Full(event)) => {
            match tokio::time::timeout(std::time::Duration::from_secs(5), tx.send(event)).await {
                Ok(Ok(())) => {
                    log::info!("[agent/approve] delayed wallet reload queued for {pubkey}");
                    true
                }
                Ok(Err(_)) => {
                    log::warn!(
                        "[agent/approve] bot channel closed before delayed wallet reload for {pubkey}"
                    );
                    false
                }
                Err(_) => {
                    log::warn!(
                        "[agent/approve] timed out queuing delayed wallet reload for {pubkey}"
                    );
                    false
                }
            }
        }
        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => false,
    }
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
    // Create channel for this connection
    let (tx, mut rx) = tokio::sync::mpsc::channel::<UpdateFrontend>(128);
    let conn_id = uuid::Uuid::new_v4();

    // Register in connections map
    {
        let mut conns = state.ws_connections.write().await;
        conns
            .entry(pubkey.clone())
            .or_default()
            .push(WsConnection { id: conn_id, tx });
    }

    info!("WebSocket connected for user {}", pubkey);

    let mut socket = socket;
    loop {
        tokio::select! {
            maybe_msg = rx.recv() => {
                let Some(msg) = maybe_msg else {
                    break;
                };

                let Ok(text) = serde_json::to_string(&msg) else {
                    log::warn!("failed to serialize websocket update for user {pubkey}");
                    continue;
                };

                match tokio::time::timeout(
                    Duration::from_secs(WS_SEND_TIMEOUT_SECS),
                    socket.send(Message::Text(text.into())),
                )
                .await
                {
                    Ok(Ok(())) => {}
                    Ok(Err(_)) => break,
                    Err(_) => {
                        log::warn!("websocket send timed out for user {pubkey}; closing connection");
                        break;
                    }
                }
            }
            inbound = socket.next() => {
                match inbound {
                    Some(Ok(Message::Ping(payload))) => {
                        match tokio::time::timeout(
                            Duration::from_secs(WS_SEND_TIMEOUT_SECS),
                            socket.send(Message::Pong(payload)),
                        )
                        .await
                        {
                            Ok(Ok(())) => {}
                            Ok(Err(_)) => break,
                            Err(_) => {
                                log::warn!("websocket pong timed out for user {pubkey}; closing connection");
                                break;
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    Some(Ok(_)) => {}
                }
            }
        };
    }

    // Cleanup: remove this sender from connections map
    {
        let mut conns = state.ws_connections.write().await;
        if let Some(senders) = conns.get_mut(&pubkey) {
            senders.retain(|conn| conn.id != conn_id);
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
        None => db_query_timeout_string(
            "backtest strategy name",
            sqlx::query_scalar::<_, String>("SELECT name FROM strategies WHERE id = $1")
                .bind(cfg.strategy_id)
                .fetch_optional(pool),
        )
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| "Unknown".to_string()),
    };

    let exchange_str = cfg.source.exchange.name();
    let market_str = cfg.source.market.as_str();

    // Insert into backtest_runs
    let run_row_id = db_query_timeout_string(
        "insert backtest_runs",
        sqlx::query_scalar::<_, sqlx::types::Uuid>(
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
        .fetch_one(pool),
    )
    .await?;

    // Insert into backtest_results
    let trades_json =
        serde_json::to_value(&result.trades).map_err(|e| format!("serialize trades: {e}"))?;
    let equity_json =
        serde_json::to_value(&result.equity_curve).map_err(|e| format!("serialize equity: {e}"))?;
    let snapshots_json =
        serde_json::to_value(&result.snapshots).map_err(|e| format!("serialize snapshots: {e}"))?;

    db_query_timeout_string(
        "insert backtest_results",
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
        .execute(pool),
    )
    .await?;

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
    let (limit, offset) = bounded_pagination(params.limit, params.offset);

    let rows = match (&params.asset, &params.strategy_id) {
        (Some(asset), Some(sid)) => {
            let strategy_id: uuid::Uuid = sid.parse().map_err(|_| StatusCode::BAD_REQUEST)?;
            db_query_timeout(
                "list_backtest_history asset strategy",
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
                .fetch_all(&state.pool),
            )
            .await?
        }
        (Some(asset), None) => {
            db_query_timeout(
                "list_backtest_history asset",
                sqlx::query_as::<_, BacktestRunRow>(
                    "SELECT * FROM backtest_runs
                 WHERE pubkey = $1 AND asset = $2
                 ORDER BY created_at DESC LIMIT $3 OFFSET $4",
                )
                .bind(&auth.pubkey)
                .bind(asset)
                .bind(limit)
                .bind(offset)
                .fetch_all(&state.pool),
            )
            .await?
        }
        (None, Some(sid)) => {
            let strategy_id: uuid::Uuid = sid.parse().map_err(|_| StatusCode::BAD_REQUEST)?;
            db_query_timeout(
                "list_backtest_history strategy",
                sqlx::query_as::<_, BacktestRunRow>(
                    "SELECT * FROM backtest_runs
                 WHERE pubkey = $1 AND strategy_id = $2
                 ORDER BY created_at DESC LIMIT $3 OFFSET $4",
                )
                .bind(&auth.pubkey)
                .bind(strategy_id)
                .bind(limit)
                .bind(offset)
                .fetch_all(&state.pool),
            )
            .await?
        }
        (None, None) => {
            db_query_timeout(
                "list_backtest_history",
                sqlx::query_as::<_, BacktestRunRow>(
                    "SELECT * FROM backtest_runs
                 WHERE pubkey = $1
                 ORDER BY created_at DESC LIMIT $2 OFFSET $3",
                )
                .bind(&auth.pubkey)
                .bind(limit)
                .bind(offset)
                .fetch_all(&state.pool),
            )
            .await?
        }
    };

    Ok(Json(rows))
}

fn bounded_pagination(limit: Option<i64>, offset: Option<i64>) -> (i64, i64) {
    let limit = limit.unwrap_or(DEFAULT_PAGE_LIMIT).clamp(1, MAX_PAGE_LIMIT);
    let offset = offset.unwrap_or(0).max(0);

    (limit, offset)
}

fn bounded_strategy_list_pagination(limit: Option<i64>, offset: Option<i64>) -> (i64, i64) {
    let limit = limit
        .unwrap_or(DEFAULT_STRATEGY_LIST_LIMIT)
        .clamp(1, MAX_STRATEGY_LIST_LIMIT);
    let offset = offset.unwrap_or(0).max(0);

    (limit, offset)
}

fn validate_market_path(market: &str) -> Result<(), StatusCode> {
    if market.is_empty()
        || market.chars().count() > MARKET_PATH_MAX_LEN
        || market.chars().any(char::is_control)
    {
        return Err(StatusCode::BAD_REQUEST);
    }

    Ok(())
}

async fn get_backtest_result(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<uuid::Uuid>,
) -> Result<impl IntoResponse, StatusCode> {
    // Verify ownership
    let owns = db_query_timeout(
        "get_backtest_result ownership",
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM backtest_runs WHERE id = $1 AND pubkey = $2)",
        )
        .bind(id)
        .bind(&auth.pubkey)
        .fetch_one(&state.pool),
    )
    .await?;

    if !owns {
        return Err(StatusCode::NOT_FOUND);
    }

    let row = db_query_timeout(
        "get_backtest_result",
        sqlx::query_as::<_, BacktestResultRow>("SELECT * FROM backtest_results WHERE run_id = $1")
            .bind(id)
            .fetch_optional(&state.pool),
    )
    .await?;

    match row {
        Some(r) => Ok(Json(r)),
        None => Err(StatusCode::NOT_FOUND),
    }
}

async fn delete_backtest_run(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(id): Path<uuid::Uuid>,
) -> impl IntoResponse {
    let result = db_query_timeout(
        "delete_backtest_run",
        sqlx::query("DELETE FROM backtest_runs WHERE id = $1 AND pubkey = $2")
            .bind(id)
            .bind(&auth.pubkey)
            .execute(&state.pool),
    )
    .await;

    match result {
        Ok(r) if r.rows_affected() > 0 => StatusCode::NO_CONTENT,
        Ok(_) => StatusCode::NOT_FOUND,
        Err(status) => status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn healthz_returns_no_content() {
        assert_eq!(healthz().await, StatusCode::NO_CONTENT);
    }

    #[test]
    fn bounded_pagination_clamps_limit_and_offset() {
        assert_eq!(bounded_pagination(None, None), (DEFAULT_PAGE_LIMIT, 0));
        assert_eq!(bounded_pagination(Some(-10), Some(-5)), (1, 0));
        assert_eq!(
            bounded_pagination(Some(MAX_PAGE_LIMIT + 1), Some(25)),
            (MAX_PAGE_LIMIT, 25)
        );
    }

    #[test]
    fn bounded_strategy_list_pagination_clamps_limit_and_offset() {
        assert_eq!(
            bounded_strategy_list_pagination(None, None),
            (DEFAULT_STRATEGY_LIST_LIMIT, 0)
        );
        assert_eq!(
            bounded_strategy_list_pagination(Some(-10), Some(-5)),
            (1, 0)
        );
        assert_eq!(
            bounded_strategy_list_pagination(Some(MAX_STRATEGY_LIST_LIMIT + 1), Some(25)),
            (MAX_STRATEGY_LIST_LIMIT, 25)
        );
    }

    #[test]
    fn validate_market_path_rejects_empty_control_and_oversized_values() {
        assert!(validate_market_path("BTC").is_ok());
        assert!(validate_market_path("").is_err());
        assert!(validate_market_path("bad\nmarket").is_err());
        assert!(validate_market_path(&"x".repeat(MARKET_PATH_MAX_LEN + 1)).is_err());
    }

    #[test]
    fn parse_cors_origins_parses_comma_separated_header_values() {
        let origins = parse_cors_origins("https://app.example, http://localhost:5173")
            .expect("origins should parse");

        assert_eq!(origins.len(), 2);
        assert_eq!(origins[0], HeaderValue::from_static("https://app.example"));
        assert_eq!(
            origins[1],
            HeaderValue::from_static("http://localhost:5173")
        );
    }

    #[test]
    fn parse_cors_origins_rejects_empty_or_invalid_values() {
        assert!(parse_cors_origins(" , ").is_err());
        assert!(parse_cors_origins("not a header\nvalue").is_err());
    }

    #[test]
    fn parse_cors_policy_requires_explicit_permissive_wildcard() {
        assert!(matches!(
            parse_cors_policy("*").expect("wildcard should parse"),
            CorsPolicy::Permissive
        ));
        assert!(parse_cors_policy("").is_err());
    }

    #[test]
    fn normalize_auth_address_rejects_invalid_and_canonicalizes_valid_address() {
        assert!(normalize_auth_address("not-address").is_err());

        assert_eq!(
            normalize_auth_address(" 0x0000000000000000000000000000000000000001 ")
                .expect("address should normalize"),
            "0x0000000000000000000000000000000000000001"
        );
    }

    #[test]
    fn prune_expired_nonces_removes_only_expired_entries() {
        let now = Instant::now();
        let mut store = std::collections::HashMap::from([
            (
                "fresh".to_string(),
                (
                    "nonce".to_string(),
                    now - Duration::from_secs(NONCE_TTL_SECS),
                ),
            ),
            (
                "expired".to_string(),
                (
                    "nonce".to_string(),
                    now - Duration::from_secs(NONCE_TTL_SECS + 1),
                ),
            ),
        ]);

        prune_expired_nonces(&mut store, now);

        assert!(store.contains_key("fresh"));
        assert!(!store.contains_key("expired"));
    }

    #[test]
    fn issue_nonce_for_address_reuses_fresh_nonce_and_prunes_expired() {
        let now = Instant::now();
        let mut store = std::collections::HashMap::from([
            (
                "0x0000000000000000000000000000000000000001".to_string(),
                ("existing".to_string(), now),
            ),
            (
                "expired".to_string(),
                (
                    "old".to_string(),
                    now - Duration::from_secs(NONCE_TTL_SECS + 1),
                ),
            ),
        ]);

        let reused = issue_nonce_for_address(
            &mut store,
            "0x0000000000000000000000000000000000000001".to_string(),
            now,
        )
        .expect("fresh nonce should be reused");
        assert_eq!(reused, "existing");
        assert!(!store.contains_key("expired"));

        let created = issue_nonce_for_address(
            &mut store,
            "0x0000000000000000000000000000000000000002".to_string(),
            now,
        )
        .expect("new nonce should be issued");
        assert_ne!(created, "existing");
    }

    #[test]
    fn verify_nonce_signature_preserves_nonce_on_bad_signature() {
        let now = Instant::now();
        let address = "0x0000000000000000000000000000000000000001";
        let mut store =
            std::collections::HashMap::from([(address.to_string(), ("nonce".to_string(), now))]);

        let result = verify_nonce_signature_and_consume(&mut store, address, "0x00", "nonce", now);

        assert_eq!(result, Err(StatusCode::UNAUTHORIZED));
        assert!(store.contains_key(address));
    }

    #[test]
    fn verify_nonce_signature_rejects_wrong_or_expired_nonce_without_auth() {
        let now = Instant::now();
        let address = "0x0000000000000000000000000000000000000001";
        let mut store =
            std::collections::HashMap::from([(address.to_string(), ("nonce".to_string(), now))]);

        assert_eq!(
            verify_nonce_signature_and_consume(&mut store, address, "0x00", "wrong", now),
            Err(StatusCode::BAD_REQUEST)
        );
        assert!(store.contains_key(address));

        let expired_at = now - Duration::from_secs(NONCE_TTL_SECS + 1);
        store.insert(address.to_string(), ("nonce".to_string(), expired_at));

        assert_eq!(
            verify_nonce_signature_and_consume(&mut store, address, "0x00", "nonce", now),
            Err(StatusCode::GONE)
        );
        assert!(!store.contains_key(address));
    }

    #[test]
    fn normalize_agent_name_prefix_defaults_trims_and_rejects_bad_values() {
        assert_eq!(
            normalize_agent_name_prefix(None).expect("default should work"),
            "kwant"
        );
        assert_eq!(
            normalize_agent_name_prefix(Some(" custom ")).expect("trimmed name should work"),
            "custom"
        );
        assert!(normalize_agent_name_prefix(Some("x".repeat(65).as_str())).is_err());
        assert!(normalize_agent_name_prefix(Some("bad\nname")).is_err());
    }

    #[test]
    fn validate_strategy_payload_bounds_rejects_oversized_strategy_inputs() {
        let mut payload = SaveStrategyPayload {
            name: "valid".to_string(),
            on_idle: String::new(),
            on_open: String::new(),
            on_busy: String::new(),
            indicators: serde_json::json!([]),
            state_declarations: None,
            is_active: Some(true),
        };

        assert!(validate_strategy_payload_bounds(&payload).is_none());

        payload.name = String::new();
        assert!(validate_strategy_payload_bounds(&payload).is_some());

        payload.name = "valid".to_string();
        payload.on_idle = "x".repeat(STRATEGY_SCRIPT_MAX_BYTES + 1);
        assert!(validate_strategy_payload_bounds(&payload).is_some());

        payload.on_idle.clear();
        payload.indicators = serde_json::Value::Array(
            (0..=STRATEGY_INDICATORS_MAX)
                .map(|_| serde_json::json!(["BTC", "Close", "1m"]))
                .collect(),
        );
        assert!(validate_strategy_payload_bounds(&payload).is_some());
    }

    #[test]
    fn normalized_strategy_name_trims_persisted_name() {
        let payload = SaveStrategyPayload {
            name: "  mean reversion  ".to_string(),
            on_idle: String::new(),
            on_open: String::new(),
            on_busy: String::new(),
            indicators: serde_json::json!([]),
            state_declarations: None,
            is_active: Some(true),
        };

        assert_eq!(normalized_strategy_name(&payload), "mean reversion");
    }

    #[tokio::test]
    async fn active_backtest_guard_releases_on_explicit_release_and_drop() {
        let active = Arc::new(tokio::sync::RwLock::new(std::collections::HashSet::new()));

        let guard = ActiveBacktestGuard::acquire(Arc::clone(&active), "user".to_string())
            .await
            .expect("first guard should acquire");
        assert!(
            ActiveBacktestGuard::acquire(Arc::clone(&active), "user".to_string())
                .await
                .is_none()
        );

        guard.release().await;
        assert!(
            ActiveBacktestGuard::acquire(Arc::clone(&active), "user".to_string())
                .await
                .is_some()
        );

        let guard = ActiveBacktestGuard::acquire(Arc::clone(&active), "drop-user".to_string())
            .await
            .expect("drop guard should acquire");
        drop(guard);
        tokio::time::sleep(Duration::from_millis(10)).await;

        assert!(!active.read().await.contains("drop-user"));
    }

    #[tokio::test]
    async fn bot_startup_guard_allows_one_builder_per_user() {
        let active = Arc::new(tokio::sync::RwLock::new(std::collections::HashSet::new()));

        let guard = BotStartupGuard::acquire(Arc::clone(&active), "user".to_string())
            .await
            .expect("first guard should acquire");
        assert!(
            BotStartupGuard::acquire(Arc::clone(&active), "user".to_string())
                .await
                .is_none()
        );

        drop(guard);
        tokio::time::sleep(Duration::from_millis(10)).await;
        assert!(
            BotStartupGuard::acquire(Arc::clone(&active), "user".to_string())
                .await
                .is_some()
        );
    }
}
