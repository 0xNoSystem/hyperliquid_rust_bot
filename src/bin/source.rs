use hyperliquid_rust_bot::Strategy;
use hyperliquid_rust_bot::backtest::{
    BacktestConfig, BacktestProgress, BacktestRunRequest, Backtester, DataSource, Exchange,
    MarketType, PositionSnapshot,
};
use hyperliquid_rust_bot::{Error, TimeFrame, get_time_now_and_candles_ago};
use std::fs::{File, create_dir_all};
use std::io::{BufWriter, Write};
use std::path::Path;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn"))
        .try_init();

    let source = DataSource::with_quote(Exchange::Binance, MarketType::Futures, "USDT");
    let (start, end) = get_time_now_and_candles_ago(1_000_000, TimeFrame::Min1);

    let request = BacktestRunRequest {
        run_id: None,
        config: BacktestConfig {
            asset: "SOL".to_string(),
            source,
            strategy: Strategy::RsiEmaScalp,
            resolution: TimeFrame::Min1,
            margin: 10_000.0,
            lev: 8,
            taker_fee_bps: 3,
            maker_fee_bps: 1,
            funding_rate_bps_per_8h: 1.0,
            start_time: start,
            end_time: end,
            snapshot_interval_candles: 10,
        },
        warmup_candles: 5000,
    };

    let mut backtester = Backtester::from_request(request);
    let result = backtester
        .run_with_progress(|progress| match progress {
            BacktestProgress::LoadingCandles { loaded, total } => {
                println!("Loading candles: {}/{}", loaded, total);
            }
            BacktestProgress::Simulating { processed, total } => {
                println!("Simulating: {}/{}", processed, total);
            }
            BacktestProgress::Failed { message } => {
                eprintln!("Backtest failed: {}", message);
            }
            _ => {}
        })
        .await?;

    let position_rows =
        write_position_states_csv(&result.snapshots, Path::new("equity/curve.csv"))?;

    println!(
        "Backtest done. Trades: {} | Net PnL: {:.4} | Snapshots: {}",
        result.summary.total_trades,
        result.summary.net_pnl,
        result.snapshots.len()
    );

    println!(
        "Saved position states to equity/curve.csv ({} rows)",
        position_rows
    );

    dbg!(&result.config);
    dbg!(&result.summary);
    if let Some(last) = result.equity_curve.last() {
        println!(
            "Final equity: {:.4} (balance {:.4}, upnl {:.4})",
            last.equity, last.balance, last.upnl
        );
    }

    Ok(())
}

fn write_position_states_csv(snapshots: &[PositionSnapshot], path: &Path) -> Result<usize, Error> {
    let Some(parent) = path.parent() else {
        return Err(Error::Custom(
            "Invalid output path for position states csv".to_string(),
        ));
    };

    create_dir_all(parent)
        .map_err(|e| Error::Custom(format!("Failed to create {}: {}", parent.display(), e)))?;

    let file = File::create(path)
        .map_err(|e| Error::Custom(format!("Failed to create {}: {}", path.display(), e)))?;
    let mut writer = BufWriter::new(file);

    writeln!(
        writer,
        "snapshot_id,snapshot_ts,reason,candle_open_time,candle_close_time,position_open_time,side,size,entry_px,fees,funding,realised_pnl,fill_type,balance,equity,upnl"
    )
    .map_err(|e| {
        Error::Custom(format!(
            "Failed to write header to {}: {}",
            path.display(),
            e
        ))
    })?;

    let mut rows = 0usize;
    for snap in snapshots {
        let Some(pos) = snap.position else {
            continue;
        };

        writeln!(
            writer,
            "{},{},{},{},{},{},{:?},{:.10},{:.10},{:.10},{:.10},{:.10},{:?},{:.10},{:.10},{:.10}",
            snap.id,
            snap.ts,
            format!("{:?}", snap.reason),
            snap.candle.open_time,
            snap.candle.close_time,
            pos.open_time,
            pos.side,
            pos.size,
            pos.entry_px,
            pos.fees,
            pos.funding,
            pos.realised_pnl,
            pos.fill_type,
            snap.balance,
            snap.equity,
            snap.upnl
        )
        .map_err(|e| {
            Error::Custom(format!(
                "Failed to write position row to {}: {}",
                path.display(),
                e
            ))
        })?;
        rows = rows.saturating_add(1);
    }

    writer
        .flush()
        .map_err(|e| Error::Custom(format!("Failed to flush {}: {}", path.display(), e)))?;

    Ok(rows)
}
