# Hyperliquid Trading Terminal — Actix → Axum + Supabase Migration

## Role

You are migrating a Rust trading terminal backend from Actix-web to Axum, adding Supabase (PostgreSQL) integration, wallet-based authentication, and multi-user/multi-device WebSocket support. You are NOT rewriting the trading logic — signal engine, executor, strategy, indicators, margin book, broadcaster, and candle cache are untouched. You are replacing the HTTP/WS transport layer and adding persistence + auth.

---

## Current Architecture (what exists)

### Entrypoint: `src/bin/kwant.rs`
- Single-user Actix-web server on `127.0.0.1:8090`
- Routes: `POST /command` (receives `BotEvent` JSON), `POST /backtest`, `GET /ws`
- WebSocket uses Actix actors (`MyWebSocket` implements `Actor`, `StreamHandler`)
- Bot commands arrive via `Sender<BotEvent>` channel
- Frontend updates broadcast via `tokio::sync::broadcast::Sender<UpdateFrontend>`
- Wallet loaded from `.env` (`PRIVATE_KEY`, `WALLET`) — single hardcoded user
- Shutdown via Ctrl+C → sends `BotEvent::Kill`, stops server

### Bot: `src/bot.rs`
- `Bot` struct holds: `InfoClient`, `Arc<Wallet>`, `HashMap<String, Sender<MarketCommand>>` (per-market channels), broadcast/cache senders, `Sender<UpdateFrontend>` (single `app_tx`)
- `Bot::new()` takes `Wallet`, `broadcast_tx`, `cache_tx` → returns `(Bot, Sender<BotEvent>)`
- `Bot::start(app_tx)` runs the main select loop:
  - Listens to user WS events (fills, funding) from Hyperliquid
  - Listens to `BotEvent` commands from the frontend
  - Listens to `MarketUpdate` from per-market tasks
- `BotEvent` enum: `AddMarket`, `ResumeMarket`, `PauseMarket`, `RemoveMarket`, `MarketComm`, `ManualUpdateMargin`, `ResumeAll`, `PauseAll`, `CloseAll`, `GetSession`, `Kill`
- `Session` = `Arc<Mutex<HashMap<String, MarketState>>>` — in-memory state
- `MarginBook` synced every 2s in a background task

### Market: `src/market.rs`
- `Market::new()` takes wallet, channels, subscription info, config → spawns signal engine + executor
- `Market::start()` runs command loop: handles leverage/strategy/indicator updates, trade results, pause/resume/close
- `MarketCommand` enum: `UpdateLeverage`, `UpdateStrategy`, `EditIndicators`, `ReceiveTrade`, `UpdateOpenPosition`, `UserEvent`, `UpdateMargin`, `UpdateIndicatorData`, `EngineStateChange`, `Resume`, `Pause`, `Close`
- `MarketState`: asset, lev, strategy, margin, pnl, is_paused, position, engine_state, trades vec

### Frontend Types: `src/frontend/ws_structs.rs`
- `UpdateFrontend` enum: all messages sent to the UI (market confirmations, price streams, indicator data, trade results, backtest progress, session load, errors, status)
- `AddMarketInfo`: asset, margin_alloc, lev, strategy, config
- `MarketInfo`: full market snapshot for UI
- `MarketStream`: price updates and indicator data

### Broadcast System: `src/broadcast/`
- `Broadcaster`: manages per-asset Hyperliquid candle subscriptions, one WS connection, fans out via `broadcast::Sender<PriceData>`
- `CandleCache`: caches candle history per timeframe per asset, handles backfill from HL API
- Both are standalone async tasks started before the bot

### Wallet: `src/wallet.rs`
- `Wallet` struct: `InfoClient`, `PrivateKeySigner`, `pubkey: String`, `url: BaseUrl`
- Methods: `get_user_fees`, `user_fills`, `get_user_margin`

---

## Target Architecture (what to build)

### Overview
```
                                    ┌─────────────────┐
  Device A ──WebSocket──┐          │   Supabase       │
  Device B ──WebSocket──┼── Axum ──┤   (PostgreSQL)   │
  Device C ──WebSocket──┘  Server  │                  │
                            │      └─────────────────┘
                            │
                    ┌───────┴────────┐
                    │  BotManager    │
                    │  (per-user     │
                    │   bot lookup)  │
                    └───────┬────────┘
                            │
              ┌─────────────┼─────────────┐
              Bot(user1)   Bot(user2)   Bot(userN)
              │             │             │
           Markets       Markets       Markets
```

### Key Changes

1. **Actix → Axum**: Replace all Actix types, actors, and routing with Axum handlers, extractors, and tower middleware
2. **Single-user → Multi-user**: BotManager maps `pubkey → Bot`, each user gets their own bot instance
3. **Wallet auth**: SIWE-style (Sign-In with Ethereum) — nonce challenge, signature verification, JWT issuance
4. **Supabase/PostgreSQL**: Persist users, strategies, trades, encrypted API keys
5. **Multi-device WebSocket**: `HashMap<String, Vec<Sender<UpdateFrontend>>>` keyed by pubkey
6. **Shared PgPool**: One connection pool passed to all bots via `Arc`

---

## Database Schema (Supabase PostgreSQL)

THESE SQL COMMANDS HAVE ALREADY BEEN RAN IN SUPABASE SQL Console. Do NOT use Supabase Auth — auth is handled entirely in Axum.

```sql
CREATE TABLE users (
  pubkey         VARCHAR(42) PRIMARY KEY
    CHECK (pubkey ~ '^0x[0-9a-f]{40}$'),
  api_key_enc    BYTEA,
  created_at     TIMESTAMPTZ DEFAULT now()
);

CREATE TABLE strategies (
  id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  pubkey      VARCHAR(42) REFERENCES users(pubkey) NOT NULL,
  name        TEXT NOT NULL,
  on_idle     TEXT NOT NULL,
  on_open     TEXT NOT NULL,
  on_busy     TEXT NOT NULL,
  indicators  JSONB NOT NULL DEFAULT '{}',
  is_active   BOOLEAN DEFAULT false,
  created_at  TIMESTAMPTZ DEFAULT now(),
  updated_at  TIMESTAMPTZ DEFAULT now()
);

CREATE TABLE trades (
  id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  pubkey      VARCHAR(42) REFERENCES users(pubkey) NOT NULL,
  market      TEXT NOT NULL,
  side        TEXT NOT NULL,
  size        FLOAT8 NOT NULL,
  pnl         FLOAT8 NOT NULL,
  total_pnl   FLOAT8 NOT NULL,
  fees        FLOAT8 NOT NULL,
  funding     FLOAT8 NOT NULL,
  open_time   BIGINT NOT NULL,
  open_price  FLOAT8 NOT NULL,
  open_type   TEXT NOT NULL,
  close_time  BIGINT NOT NULL,
  close_price FLOAT8 NOT NULL,
  close_type  TEXT NOT NULL
);

CREATE INDEX idx_trades_pubkey_market ON trades(pubkey, market);
CREATE INDEX idx_strategies_pubkey ON strategies(pubkey);
```

---

## Implementation Plan — Execute in Order

### Phase 1: Dependencies

Update `Cargo.toml`. Remove: `actix`, `actix-web`, `actix-web-actors`, `actix-cors`. Add:

```toml
axum = { version = "0.8", features = ["ws", "macros"] }
axum-extra = { version = "0.12", features = ["typed-header"] }
tower = "0.5"
tower-http = { version = "0.6", features = ["cors", "trace"] }
sqlx = { version = "0.8", features = ["runtime-tokio", "tls-rustls", "postgres", "uuid", "chrono", "json"] }
jsonwebtoken = "9"
aes-gcm = "0.10"
rand = "0.8"
```

Keep all existing dependencies that are still used (tokio, serde, serde_json, flume, hyperliquid_rust_sdk, kwant, etc).

### Phase 2: Shared Application State

Create `src/app_state.rs`:

```rust
use std::sync::Arc;
use tokio::sync::{RwLock, mpsc::Sender};
use sqlx::PgPool;
use std::collections::HashMap;
use crate::UpdateFrontend;

pub type WsConnections = Arc<RwLock<HashMap<String, Vec<Sender<UpdateFrontend>>>>>;

pub struct AppState {
    pub pool: PgPool,
    pub ws_connections: WsConnections,
    pub bot_manager: Arc<RwLock<BotManager>>,
    pub jwt_secret: String,
    pub encryption_key: [u8; 32],
}
```

`BotManager` is a new struct (see Phase 5).

### Phase 3: Wallet Authentication

Create `src/auth.rs`. The auth flow:

1. `GET /auth/nonce?address=0x...` → backend generates random nonce, stores in `HashMap<String, (String, Instant)>` (address → nonce + expiry), returns nonce
2. `POST /auth/verify` with body `{ address, signature, nonce }` → backend verifies EIP-191 signature using `alloy` (already a dependency via hyperliquid_rust_sdk), checks nonce matches, creates/fetches user in DB, issues JWT
3. JWT contains: `sub` (pubkey), `exp` (expiry), `iat` (issued at)
4. JWT verification middleware extracts pubkey from token on every authenticated request

```rust
// Axum extractor for authenticated routes
pub struct AuthUser {
    pub pubkey: String,
}

#[async_trait]
impl<S> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
{
    type Rejection = StatusCode;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        // Extract JWT from Authorization header or query param (for WebSocket)
        // Verify with jsonwebtoken
        // Return AuthUser { pubkey }
    }
}
```

For nonce storage, use an in-memory `Arc<RwLock<HashMap<String, (String, Instant)>>>` with a background task that prunes expired nonces every 60s. Nonces expire after 5 minutes.

Signature verification: use `alloy::primitives::Address` and `alloy::signers` to recover the signer address from the EIP-191 personal_sign message. The message format is: `"Sign this message to authenticate with Hyperliquid Terminal.\n\nNonce: {nonce}"`.

### Phase 4: API Key Encryption

Create `src/crypto.rs`:

```rust
use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::Aead};
use rand::RngCore;

pub fn encrypt(master_key: &[u8; 32], plaintext: &[u8]) -> Vec<u8> {
    let cipher = Aes256Gcm::new(master_key.into());
    let mut nonce_bytes = [0u8; 12];
    rand::rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher.encrypt(nonce, plaintext).expect("encryption failed");
    [nonce_bytes.as_slice(), &ciphertext].concat()
}

pub fn decrypt(master_key: &[u8; 32], stored: &[u8]) -> Result<Vec<u8>, aes_gcm::Error> {
    let cipher = Aes256Gcm::new(master_key.into());
    let (nonce_bytes, ciphertext) = stored.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);
    cipher.decrypt(nonce, ciphertext)
}
```

### Phase 5: BotManager

Create `src/bot_manager.rs`. This replaces the single-bot setup in `kwant.rs`:

```rust
pub struct BotManager {
    bots: HashMap<String, BotHandle>,  // pubkey → handle
    broadcast_tx: UnboundedSender<BroadcastCmd>,
    cache_tx: Sender<CacheCmdIn>,
}

struct BotHandle {
    cmd_tx: Sender<BotEvent>,
    // cancel token for shutdown
}

impl BotManager {
    /// Look up or create a bot for the given user
    pub async fn get_or_create_bot(&mut self, pubkey: &str, pool: &PgPool, encryption_key: &[u8; 32]) -> Result<Sender<BotEvent>, Error> {
        if let Some(handle) = self.bots.get(pubkey) {
            return Ok(handle.cmd_tx.clone());
        }

        // 1. Fetch user's encrypted API key from DB
        // 2. Decrypt it
        // 3. Create Wallet from the decrypted key
        // 4. Create Bot::new(wallet, broadcast_tx.clone(), cache_tx.clone())
        // 5. Spawn bot.start(...)
        // 6. Store handle
        // 7. Load active strategies from DB: SELECT * FROM strategies WHERE pubkey = $1 AND is_active = true
        // 8. For each active strategy, send BotEvent::AddMarket
    }

    pub fn remove_bot(&mut self, pubkey: &str) { ... }
}
```

The `Broadcaster` and `CandleCache` remain shared across ALL users (one Hyperliquid market data connection). Each bot subscribes to assets via the shared `broadcast_tx`.

### Phase 6: WebSocket Handler (Axum)

Replace the Actix actor-based WebSocket with Axum's native WebSocket support:

```rust
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    auth: AuthUser,  // extracted from query param ?token=... for WS
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

    // Send initial state snapshot
    // Query bot for current session state and send it

    // Spawn task: channel → WebSocket (outbound)
    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if let Ok(text) = serde_json::to_string(&msg) {
                if ws_sender.send(axum::extract::ws::Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
        }
    });

    // Receive task: WebSocket → handle pings, commands (inbound)
    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_receiver.next().await {
            match msg {
                axum::extract::ws::Message::Ping(data) => { /* pong handled automatically by axum */ }
                axum::extract::ws::Message::Close(_) => break,
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
}
```

### Phase 7: Broadcasting to Multi-Device

Replace the current `app_tx: Option<Sender<UpdateFrontend>>` pattern. The bot no longer holds a direct sender to the frontend. Instead, each bot broadcasts via the shared `WsConnections` map:

```rust
pub async fn broadcast_to_user(
    conns: &WsConnections,
    pubkey: &str,
    msg: UpdateFrontend,
) {
    let conns = conns.read().await;
    if let Some(senders) = conns.get(pubkey) {
        for tx in senders {
            let _ = tx.try_send(msg.clone());
        }
    }
}
```

Modify `Bot::start()` signature: instead of taking `Sender<UpdateFrontend>`, it takes `WsConnections` and the bot's `pubkey`. Everywhere it currently does `app_tx.send(...)` or `app_tx.try_send(...)`, replace with `broadcast_to_user(&ws_connections, &pubkey, msg).await`.

The `MarketUpdate` → `UpdateFrontend` relay task inside `Bot::start()` also uses `broadcast_to_user` instead of a single sender.

### Phase 8: REST Routes (Axum)

Create `src/routes.rs`:

```rust
pub fn create_router(state: Arc<AppState>) -> Router {
    Router::new()
        // Auth (unauthenticated)
        .route("/auth/nonce", get(get_nonce))
        .route("/auth/verify", post(verify_signature))
        // Bot commands (authenticated)
        .route("/command", post(execute_command))
        .route("/backtest", post(run_backtest))
        // Data queries (authenticated)
        .route("/trades/:market", get(get_trades))
        .route("/strategies", get(list_strategies).post(save_strategy))
        .route("/strategies/:id", put(update_strategy).delete(delete_strategy))
        .route("/api-key", post(set_api_key))
        // WebSocket
        .route("/ws", get(ws_handler))
        // Middleware
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
```

#### `POST /command`
Same logic as current `execute()` handler but with auth:
```rust
async fn execute_command(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(event): Json<BotEvent>,
) -> impl IntoResponse {
    let manager = state.bot_manager.read().await;
    if let Some(handle) = manager.get_bot(&auth.pubkey) {
        match handle.cmd_tx.try_send(event) {
            Ok(()) => StatusCode::OK,
            Err(TrySendError::Full(_)) => StatusCode::TOO_MANY_REQUESTS,
            Err(TrySendError::Closed(_)) => StatusCode::SERVICE_UNAVAILABLE,
        }
    } else {
        StatusCode::NOT_FOUND  // no active bot for this user
    }
}
```

#### `GET /trades/:market`
Lazy-loaded trade history:
```rust
async fn get_trades(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Path(market): Path<String>,
    Query(params): Query<TradeQueryParams>,  // limit, offset or cursor
) -> impl IntoResponse {
    let trades = sqlx::query_as::<_, TradeRow>(
        "SELECT * FROM trades WHERE pubkey = $1 AND market = $2 ORDER BY close_time DESC LIMIT $3 OFFSET $4"
    )
    .bind(&auth.pubkey)
    .bind(&market)
    .bind(params.limit.unwrap_or(50))
    .bind(params.offset.unwrap_or(0))
    .fetch_all(&state.pool)
    .await;

    // Convert TradeRow → TradeInfo, return as JSON
}
```

#### `POST /api-key`
```rust
async fn set_api_key(
    State(state): State<Arc<AppState>>,
    auth: AuthUser,
    Json(payload): Json<SetApiKeyPayload>,  // { api_key: String }
) -> impl IntoResponse {
    let encrypted = crate::crypto::encrypt(&state.encryption_key, payload.api_key.as_bytes());

    sqlx::query("UPDATE users SET api_key_enc = $1 WHERE pubkey = $2")
        .bind(&encrypted)
        .bind(&auth.pubkey)
        .execute(&state.pool)
        .await
        .map(|_| StatusCode::OK)
        .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}
```

### Phase 9: Trade Persistence

When a bot writes a trade (currently `MarketCommand::ReceiveTrade`), also insert into the database. In `market.rs`, in the `ReceiveTrade` handler:

```rust
MarketCommand::ReceiveTrade(trade_info) => {
    self.trade_history.push(trade_info);

    // Existing: relay to frontend
    let _ = bot_update_tx.send(MarketUpdate::MarketInfoUpdate((
        asset.name.clone(),
        EditMarketInfo::Trade(trade_info),
    )));

    // NEW: persist to DB
    let pool = self.pool.clone();  // Market needs a PgPool handle
    let pubkey = self.pubkey.clone();
    let market_name = asset.name.clone();
    tokio::spawn(async move {
        let _ = sqlx::query(
            "INSERT INTO trades (pubkey, market, side, size, pnl, total_pnl, fees, funding, open_time, open_price, open_type, close_time, close_price, close_type)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)"
        )
        .bind(&pubkey)
        .bind(&market_name)
        .bind(format!("{:?}", trade_info.side))
        .bind(trade_info.size)
        .bind(trade_info.pnl)
        .bind(trade_info.total_pnl)
        .bind(trade_info.fees)
        .bind(trade_info.funding)
        .bind(trade_info.open.time as i64)
        .bind(trade_info.open.price)
        .bind(format!("{:?}", trade_info.open.fill_type))
        .bind(trade_info.close.time as i64)
        .bind(trade_info.close.price)
        .bind(format!("{:?}", trade_info.close.fill_type))
        .execute(&pool)
        .await;
    });
}
```

This means `Market::new()` needs to accept `PgPool` and `pubkey: String` as additional parameters. Thread them through from `Bot::add_market()`.

### Phase 10: New Entrypoint

Replace `src/bin/kwant.rs` entirely:

```rust
use axum::Router;
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv::dotenv().ok();
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

    // Shared infrastructure (one instance, all users)
    let url = BaseUrl::Mainnet;
    let (mut candle_cache, cache_tx) = CandleCache::new(url).await?;
    let (mut broadcaster, broadcast_tx) = Broadcaster::new(url, cache_tx.clone()).await?;
    tokio::spawn(async move { candle_cache.start().await });
    tokio::spawn(async move { broadcaster.start().await });

    let bot_manager = BotManager::new(broadcast_tx, cache_tx);

    let state = Arc::new(AppState {
        pool,
        ws_connections: Arc::new(RwLock::new(HashMap::new())),
        bot_manager: Arc::new(RwLock::new(bot_manager)),
        jwt_secret,
        encryption_key: encryption_key.try_into().unwrap(),
    });

    let app = create_router(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8090").await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c().await.ok();
}
```

### Environment Variables

`.env` file now requires:
```
DATABASE_URL=postgresql://postgres:password@db.xxxx.supabase.co:5432/postgres
JWT_SECRET=<random 64+ char string>
ENCRYPTION_KEY=<64 hex chars = 32 bytes>
```

`PRIVATE_KEY`, `WALLET`, and `AGENT_KEY` are removed from `.env`. Users provide their API keys through the UI, stored encrypted in the database.

---

## What NOT to Change

These modules are untouched — do not modify their internal logic:

- `src/signal/` — signal engine, indicators, engine commands
- `src/exec/` — executor, order types, fill processing
- `src/strategy.rs` — Strat trait, Intent, Order, etc.
- `src/broadcast/broadcaster.rs` — Hyperliquid feed management
- `src/broadcast/candle_cache.rs` — candle caching and backfill
- `src/margin.rs` — MarginBook logic (but it now gets created per-user in BotManager)
- `src/trade_setup.rs` — TimeFrame definitions
- `src/assets.rs` — market list
- `src/helper.rs` — utility functions
- `src/wallet.rs` — Wallet struct (but instantiation moves from .env to DB)

The only changes to `bot.rs` and `market.rs` are:
1. `Bot::start()` takes `WsConnections` + `pubkey` instead of `Sender<UpdateFrontend>`
2. All `app_tx.send()`/`try_send()` calls become `broadcast_to_user()`
3. `Market::new()` additionally takes `PgPool` and `pubkey: String`
4. `ReceiveTrade` handler adds a DB insert spawn

---

## Constraints

- Use `sqlx` with compile-time query checking where possible (`sqlx::query_as!` macro), fall back to runtime `sqlx::query` if type mapping is complex
- Use `axum 0.8` patterns (State extractor, not Extension)
- WebSocket: use `axum::extract::ws`, NOT a separate crate
- All DB writes for trades are fire-and-forget spawned tasks (don't block the trading loop)
- JWT tokens expire after 24 hours
- Nonces expire after 5 minutes
- The server binds to `0.0.0.0:8090` (same port as current)
- CORS: permissive for now (same as current `Cors::default().allow_any_origin()`)
- Do not introduce Redis — all session state is in-memory or in Postgres
- Do not modify the frontend React code (keep the same WS message shapes — `UpdateFrontend` enum serialization must remain identical)
- Keep `UpdateFrontend`, `BotEvent`, `AddMarketInfo`, `MarketInfo`, `MarketStream`, and all other serde-serialized types exactly as they are
- The `backtest` route handler logic stays the same, just adapted to Axum extractors

---

## File Structure After Migration

```
src/
├── bin/
│   └── kwant.rs              # NEW: Axum entrypoint
├── app_state.rs              # NEW: shared state
├── auth.rs                   # NEW: nonce, verify, JWT, AuthUser extractor
├── crypto.rs                 # NEW: AES-256-GCM encrypt/decrypt
├── bot_manager.rs            # NEW: per-user bot lifecycle
├── routes.rs                 # NEW: all Axum route handlers
├── db.rs                     # NEW: TradeRow, DB query helpers
├── bot.rs                    # MODIFIED: multi-device broadcast
├── market.rs                 # MODIFIED: trade persistence
├── broadcast/                # UNCHANGED
├── signal/                   # UNCHANGED
├── exec/                     # UNCHANGED (exec.rs or exec/)
├── frontend/                 # UNCHANGED (ws_structs, backtest_structs)
├── margin.rs                 # UNCHANGED
├── strategy.rs               # UNCHANGED
├── wallet.rs                 # UNCHANGED
├── helper.rs                 # UNCHANGED
├── assets.rs                 # UNCHANGED
├── consts.rs                 # UNCHANGED
├── trade_setup.rs            # UNCHANGED
└── lib.rs                    # MODIFIED: add new modules
```

---

## Verification Checklist

After implementation, verify:

1. `cargo check` compiles with no errors
2. All `UpdateFrontend` variants serialize to the same JSON shape as before
3. `BotEvent` deserialization from the frontend still works
4. WebSocket connects with `?token=<jwt>` query parameter
5. `POST /auth/nonce` returns a nonce for a given address
6. `POST /auth/verify` returns a JWT on valid signature
7. `POST /command` rejects requests without valid JWT
8. `GET /trades/BTC` returns paginated trade history from DB
9. Completed trades appear in both the WebSocket stream AND the database
10. Multiple WebSocket connections from the same pubkey all receive updates
11. Bot auto-recovers active strategies from DB on creation (`is_active = true`)
12. Graceful shutdown: Ctrl+C → all bots close positions → server stops


NOTE: Do not add market: String, to TradeInfo, Bot already receives a (String, TradeInfo)
