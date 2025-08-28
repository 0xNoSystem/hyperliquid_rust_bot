cargo run --bin load #update tradable assets list

cd src/bin/actix-back || exit 1

cargo run --release & #PREFIX WITH "RUST_LOG=info" for logging

cd ../../frontend/interface

npm run dev



