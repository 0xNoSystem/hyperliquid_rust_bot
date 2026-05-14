use std::env;
use std::time::Duration;

use tokio::sync::mpsc::{channel, error::TrySendError};
use tokio::task::JoinHandle;
use tokio::time::Instant;

#[derive(Clone, Copy, Debug)]
struct Config {
    bots: usize,
    markets_per_bot: usize,
    ticks: usize,
    account_events: usize,
    queue_capacity: usize,
    slow_every: usize,
    slow_delay: Duration,
}

#[derive(Debug)]
struct ScenarioStats {
    name: &'static str,
    attempted: usize,
    accepted: usize,
    dropped: usize,
    consumed: usize,
    elapsed: Duration,
}

#[tokio::main]
async fn main() -> Result<(), String> {
    let args = env::args().collect::<Vec<_>>();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        return Ok(());
    }

    let cfg = Config::from_args(&args)?;
    println!("load config: {cfg:?}");

    let price_stats = stress_price_routing(cfg).await?;
    print_stats(&price_stats);

    let account_stats = stress_account_event_fanout(cfg).await?;
    print_stats(&account_stats);

    Ok(())
}

impl Config {
    fn from_args(args: &[String]) -> Result<Self, String> {
        let queue_capacity = arg_usize(args, "queue", 256)?;
        if queue_capacity == 0 {
            return Err("--queue must be at least 1".to_string());
        }

        Ok(Self {
            bots: arg_usize(args, "bots", 100)?,
            markets_per_bot: arg_usize(args, "markets-per-bot", 3)?,
            ticks: arg_usize(args, "ticks", 2_000)?,
            account_events: arg_usize(args, "account-events", 2_000)?,
            queue_capacity,
            slow_every: arg_usize(args, "slow-every", 13)?,
            slow_delay: Duration::from_micros(arg_u64(args, "slow-delay-us", 500)?),
        })
    }
}

async fn stress_price_routing(cfg: Config) -> Result<ScenarioStats, String> {
    let market_count = cfg
        .bots
        .checked_mul(cfg.markets_per_bot)
        .ok_or_else(|| "bots * markets-per-bot overflowed".to_string())?;
    let mut senders = Vec::with_capacity(market_count);
    let mut consumers = Vec::with_capacity(market_count);

    for index in 0..market_count {
        let (tx, rx) = channel::<u64>(cfg.queue_capacity);
        senders.push(tx);
        consumers.push(spawn_counter(
            "price route",
            rx,
            is_slow(index, cfg.slow_every),
            cfg.slow_delay,
        ));
    }

    let start = Instant::now();
    let mut accepted = 0;
    let mut dropped = 0;

    for tick in 0..cfg.ticks {
        for tx in &senders {
            match tx.try_send(tick as u64) {
                Ok(()) => accepted += 1,
                Err(TrySendError::Full(_)) => dropped += 1,
                Err(TrySendError::Closed(_)) => {
                    return Err("price route channel closed during load run".to_string());
                }
            }
        }

        if tick % 256 == 0 {
            tokio::task::yield_now().await;
        }
    }

    drop(senders);
    let consumed = join_consumers(consumers).await?;

    Ok(ScenarioStats {
        name: "price routing",
        attempted: cfg.ticks * market_count,
        accepted,
        dropped,
        consumed,
        elapsed: start.elapsed(),
    })
}

async fn stress_account_event_fanout(cfg: Config) -> Result<ScenarioStats, String> {
    let mut senders = Vec::with_capacity(cfg.bots);
    let mut consumers = Vec::with_capacity(cfg.bots);

    for index in 0..cfg.bots {
        let (tx, rx) = channel::<u64>(cfg.queue_capacity);
        senders.push(tx);
        consumers.push(spawn_counter(
            "account event subscriber",
            rx,
            is_slow(index, cfg.slow_every),
            cfg.slow_delay,
        ));
    }

    let start = Instant::now();
    let mut accepted = 0;
    let mut dropped = 0;

    for event_id in 0..cfg.account_events {
        for tx in &senders {
            match tx.try_send(event_id as u64) {
                Ok(()) => accepted += 1,
                Err(TrySendError::Full(_)) => dropped += 1,
                Err(TrySendError::Closed(_)) => {
                    return Err("account event channel closed during load run".to_string());
                }
            }
        }

        if event_id % 256 == 0 {
            tokio::task::yield_now().await;
        }
    }

    drop(senders);
    let consumed = join_consumers(consumers).await?;

    Ok(ScenarioStats {
        name: "account event fanout",
        attempted: cfg.account_events * cfg.bots,
        accepted,
        dropped,
        consumed,
        elapsed: start.elapsed(),
    })
}

fn spawn_counter(
    label: &'static str,
    mut rx: tokio::sync::mpsc::Receiver<u64>,
    slow: bool,
    slow_delay: Duration,
) -> JoinHandle<Result<usize, String>> {
    tokio::spawn(async move {
        let mut consumed = 0;
        while rx.recv().await.is_some() {
            consumed += 1;
            if slow {
                tokio::time::sleep(slow_delay).await;
            }
        }

        if consumed == 0 {
            return Err(format!("{label} consumed no messages"));
        }

        Ok(consumed)
    })
}

async fn join_consumers(
    consumers: Vec<JoinHandle<Result<usize, String>>>,
) -> Result<usize, String> {
    let mut consumed = 0;

    for consumer in consumers {
        consumed += consumer
            .await
            .map_err(|err| format!("consumer task failed: {err}"))??;
    }

    Ok(consumed)
}

fn is_slow(index: usize, slow_every: usize) -> bool {
    slow_every != 0 && index.is_multiple_of(slow_every)
}

fn print_stats(stats: &ScenarioStats) {
    let seconds = stats.elapsed.as_secs_f64().max(f64::EPSILON);
    let attempted_per_second = stats.attempted as f64 / seconds;

    println!(
        "{}: attempted={} accepted={} dropped={} consumed={} elapsed={:.3}s attempted_per_sec={:.0}",
        stats.name,
        stats.attempted,
        stats.accepted,
        stats.dropped,
        stats.consumed,
        seconds,
        attempted_per_second
    );
}

fn arg_usize(args: &[String], name: &str, default: usize) -> Result<usize, String> {
    arg_value(args, name)
        .map(|value| {
            value
                .parse()
                .map_err(|err| format!("invalid --{name}: {err}"))
        })
        .unwrap_or(Ok(default))
}

fn arg_u64(args: &[String], name: &str, default: u64) -> Result<u64, String> {
    arg_value(args, name)
        .map(|value| {
            value
                .parse()
                .map_err(|err| format!("invalid --{name}: {err}"))
        })
        .unwrap_or(Ok(default))
}

fn arg_value<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    let eq_prefix = format!("--{name}=");
    let flag = format!("--{name}");
    args.iter().enumerate().find_map(|(index, arg)| {
        arg.strip_prefix(&eq_prefix).or_else(|| {
            (arg == &flag)
                .then(|| args.get(index + 1))
                .flatten()
                .map(String::as_str)
        })
    })
}

fn print_help() {
    println!(
        "Synthetic backend load harness\n\
         \n\
         Options:\n\
         --bots=<n>              default 100\n\
         --markets-per-bot=<n>   default 3\n\
         --ticks=<n>             default 2000\n\
         --account-events=<n>    default 2000\n\
         --queue=<n>             default 256\n\
         --slow-every=<n>        default 13, use 0 to disable slow consumers\n\
         --slow-delay-us=<n>     default 500"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_equals_and_space_separated_flags() {
        let args = vec![
            "load".to_string(),
            "--bots".to_string(),
            "12".to_string(),
            "--markets-per-bot=4".to_string(),
            "--account-events".to_string(),
            "8".to_string(),
            "--queue=9".to_string(),
        ];

        let cfg = Config::from_args(&args).expect("config should parse");

        assert_eq!(cfg.bots, 12);
        assert_eq!(cfg.markets_per_bot, 4);
        assert_eq!(cfg.account_events, 8);
        assert_eq!(cfg.queue_capacity, 9);
    }
}
