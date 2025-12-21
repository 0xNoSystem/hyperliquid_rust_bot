#!/bin/bash
set -e

cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
cargo test

echo "CI checks passed successfully."

