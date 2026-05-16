#!/usr/bin/env bash
set -euo pipefail

RUSTC_WRAPPER= RUST_LOG=info cargo run --release --bin kwant & #PREFIX WITH "RUST_LOG=info" for logging

cd ./web_ui

bun install
bun run dev --host



