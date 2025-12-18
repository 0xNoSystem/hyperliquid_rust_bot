# Hyperliquid Rust Bot

A Hyperliquid trading terminal (not a library) built with Rust and React/TS. It automates indicator-driven strategies, orchestrates margin across markets, and ships with a beta backtesting view (Binance candles for history, live trading on Hyperliquid).

<img width="2090" height="1277" alt="image" src="https://github.com/user-attachments/assets/d1ed699f-a1ef-48d9-882e-63291ae9a8c3" />

<img width="2506" height="1251" alt="image" src="https://github.com/user-attachments/assets/0a27d4db-1b79-4c36-b00e-3b3da6fbfb38" />

## What it does

- Manage multiple Hyperliquid markets with a margin book that syncs on-chain balances before allocating size to the bot.
- Automated signals from `kwant` indicators (RSI, StochRSI, EMA cross, ADX, ATR, SMA/EMA) with per-market timeframes and strategy presets (risk/style/stance/follow trend).
- React dashboard to add/pause/close markets, cache setups locally, view PnL/trades, and inspect indicator values in real time.
- Backtesting (beta) pulls historical candles from Binance to bypass Hyperliquid’s 5k-candle history cap; only live trading touches Hyperliquid.
- Actix backend (`src/bin/kwant.rs`) exposes `POST /command` + `ws://localhost:8090/ws` to the UI and drives order flow via `hyperliquid_rust_sdk`.
- Designed for dedicated bot accounts: manual positions on the same wallet can block markets because the bot keeps margin in sync with on-chain state.

## Requirements

- Rust toolchain (stable).
- Bun (or Node.js) for the React/Vite frontend; `run.sh` uses Bun.
- Hyperliquid API private key and wallet address; optional agent key.

## Setup

1. Clone and enter the repo:

   ```bash
   git clone <repo-url>
   cd hyperliquid_rust_bot
   ```

2. Create a `.env` in the project root (loaded by the Actix backend):

   ```env
   PRIVATE_KEY=<your API private key> # https://app.hyperliquid.xyz/API
   AGENT_KEY=<optional agent api public key>
   WALLET=<public wallet address>
   ```

   Use a wallet that is not traded manually so the bot fully controls margin.

3. Make the runner executable if needed:

   ```bash
   chmod +x ./run.sh
   ```

## Run

Start everything with one command (backend + frontend):

```bash
./run.sh
```

- Backend: `cargo run --release --bin kwant` at `http://127.0.0.1:8090` (Actix, WebSocket at `/ws`, logs via `RUST_LOG=info`).
- Frontend: Vite dev server via Bun (`http://localhost:5173` by default).
- To target testnet/local, change `BaseUrl::Mainnet` in `src/bin/kwant.rs` before running.

## Backend / frontend layout

- `src/bin/kwant.rs` – Actix entrypoint; loads `.env`, spins up the bot, exposes `/command` and `/ws`.
- `src/` – margin book (`margin.rs`), markets and signal engine (`market.rs`, `signal/`), strategy (`strategy.rs`), executor, wallet helpers, and a backtester scaffold.
- `web_ui/` – React + TypeScript + Vite + Tailwind/MUI interface (markets, per-asset detail, settings, backtest). Backtesting candles come from Binance; live trading and margin updates stream from the Actix server.

## Notes

- You can create your own strategy by looking at the Strategy trait in src/strategy.rs and reading other implemented strategies. Make sure you add it to the Strategy type in web_ui/src/types.ts (in pascalCase) and in src/components/AddMarke.tsx, i realise this is quite complicated, i'm working on making custom strategies easier to integrate. 
- Backtesting is in devolpment and purely uses Binance OHLCV; live trades execute on Hyperliquid.
- Manual trades on the same account can interfere with the bot’s margin orchestration; a dedicated account is recommended.
- Experimental software; use at your own risk.
