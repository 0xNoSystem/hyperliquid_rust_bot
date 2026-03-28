use hyperliquid_rust_bot::{IndexId, IndicatorKind, TimeFrame};

fn main() {
    let ids: Vec<IndexId> = vec![
        (IndicatorKind::Rsi(14), TimeFrame::Min15),
        (
            IndicatorKind::SmaOnRsi {
                periods: 10,
                smoothing_length: 1,
            },
            TimeFrame::Min15,
        ),
    ];
    println!("{}", serde_json::to_string_pretty(&ids).unwrap());
}
