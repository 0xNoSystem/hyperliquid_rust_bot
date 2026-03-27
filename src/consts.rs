pub const MAX_HISTORY: usize = 10000;
pub const MAX_TRADES: usize = 50;
pub const MAX_DECIMALS: u32 = 6;
pub const MIN_ORDER_VALUE: f64 = 10.0; //USDC
pub const MAX_DISCONNECTION_WINDOW: u128 = 120_000; //2min
pub const HL_MAX_CANDLES: u64 = 5000;

pub const PX_DECIMAL_ANOMALY: [&str; 3] = ["SOL", "ZEC", "BCH"];
