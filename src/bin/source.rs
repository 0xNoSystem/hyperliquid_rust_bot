use hyperliquid_rust_bot::backtest::{Backtester, DataSource, Exchange, Fetcher, MarketType};
use hyperliquid_rust_bot::{Error, OpenPosInfo, PositionOp, Side, TimeFrame, get_time_now_and_candles_ago};
use hyperliquid_rust_bot::Strategy;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let source = DataSource::with_quote(Exchange::Binance, MarketType::Futures, "USDT");
    let mut fetcher = Fetcher::new(source);

    let mut engine = Backtester::new(10000.0, 10, Strategy::ElderTripleScreen);

    let (start, end) = get_time_now_and_candles_ago(1000, TimeFrame::Day1);
    let prices = fetcher.fetch("Btc", TimeFrame::Min1, start, end).await?;

    for p in prices.iter() {
        let r = engine.tick(*p);

        if let Some(order) = r {
            let open_pos = match order.action {
                PositionOp::OpenLong => Some(OpenPosInfo {
                    side: Side::Long,
                    size: order.size,
                    entry_px: p.close,
                    open_time: p.open_time,
                }),
                PositionOp::OpenShort => Some(OpenPosInfo {
                    side: Side::Short,
                    size: order.size,
                    entry_px: p.close,
                    open_time: p.open_time,
                }),
                PositionOp::Close => None,
            };
            if let Some(trade) = engine.update_open_pos(open_pos) {
                println!("{:?}", trade);
            }
        }
    }

    Ok(())
}
