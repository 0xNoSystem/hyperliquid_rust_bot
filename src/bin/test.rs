use hyperliquid_rust_bot::{IndexId, IndicatorKind, TimeFrame};
use std::sync::Arc;

fn main() {
    let asset: Arc<str> = Arc::from("BTC");
    let ids: Vec<IndexId> = vec![
        (Arc::clone(&asset), IndicatorKind::Rsi(14), TimeFrame::Min15),
        (
            Arc::clone(&asset),
            IndicatorKind::SmaOnRsi {
                periods: 10,
                smoothing_length: 1,
            },
            TimeFrame::Min15,
        ),
    ];
    println!("{}", serde_json::to_string_pretty(&ids).unwrap());
}
