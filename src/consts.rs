pub const MAX_HISTORY: usize = 10000;
pub const MAX_TRADES: usize = 50;
pub const MAX_DECIMALS: u32 = 6;
pub const MIN_ORDER_VALUE: f64 = 10.0; //USDC
pub const MAX_DISCONNECTION_WINDOW: u128 = 120_000; //2min
pub const HL_MAX_CANDLES: u64 = 5000;

pub const PX_DECIMAL_ANOMALY: [&str; 3] = ["SOL", "ZEC", "BCH"];
pub const DEFAULT_BUILDER_ADDRESS: &str = "0x8b56d7FBC8ad2a90E1C1366CA428efb4b5Bed18F";
pub const DEFAULT_BUILDER_FEE: u64 = 50;

use std::sync::LazyLock;

pub static BUILDER: LazyLock<hyperliquid_rust_sdk::BuilderInfo> =
    LazyLock::new(|| hyperliquid_rust_sdk::BuilderInfo {
        builder: String::from(DEFAULT_BUILDER_ADDRESS),
        fee: DEFAULT_BUILDER_FEE,
    });
