#!/bin/bash
set -e

export CARGO_BUILD_RUSTC_WRAPPER="${CARGO_BUILD_RUSTC_WRAPPER:-}"

cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
cargo test

cd ./web_ui
bun run lint
bun run build

echo "CI checks passed successfully."
