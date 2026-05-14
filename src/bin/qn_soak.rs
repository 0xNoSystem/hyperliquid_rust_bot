use std::collections::HashSet;
use std::env;
use std::time::Duration;

use alloy::primitives::Address;
use futures_util::{SinkExt, StreamExt};
use hyperliquid_rust_bot::Error;
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio::time::{Instant, timeout};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async, tungstenite::protocol::Message,
};

const DEFAULT_SOAK_SECS: u64 = 60;
const QUICKNODE_WS_PATH: &str = "/hypercore/ws";
const QUICKNODE_NUMBERED_ENDPOINTS: usize = 10;
const QUICKNODE_CONNECT_TIMEOUT_SECS: u64 = 10;
const QUICKNODE_SEND_TIMEOUT_SECS: u64 = 5;
const QUICKNODE_ACK_DRAIN_TIMEOUT_SECS: u64 = 3;
const QUICKNODE_ACCOUNT_EVENT_TYPES: &[&str] = &[
    "funding",
    "CDeposit",
    "CWithdrawal",
    "cDeposit",
    "cWithdrawal",
    "deposit",
    "withdraw",
    "internalTransfer",
    "subAccountTransfer",
    "ledgerLiquidation",
    "liquidation",
    "vaultDeposit",
    "vaultCreate",
    "vaultDistribution",
    "vaultWithdraw",
    "vaultLeaderCommission",
    "accountClassTransfer",
    "spotTransfer",
    "spotGenesis",
];
type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[derive(Clone, Copy, Debug)]
struct SoakConfig {
    soak_secs: u64,
    reconnect_every: Option<Duration>,
    churn_every: Option<Duration>,
    require_events: bool,
    require_account_events: bool,
}

#[derive(Debug)]
struct ActiveFilters {
    fills: String,
    account_events: String,
}

#[derive(Debug, Default)]
struct SoakStats {
    sent_requests: u64,
    acks: u64,
    unexpected_acks: u64,
    errors: u64,
    fills: u64,
    account_events: u64,
    unknown: u64,
    non_json: u64,
    reconnects: u64,
    churns: u64,
    pending_rpc_ids: HashSet<u64>,
}

impl SoakStats {
    fn ensure_success(&self, config: &SoakConfig) -> Result<(), Error> {
        if self.errors > 0 {
            return Err(Error::Custom(format!(
                "QuickNode soak received {} JSON-RPC error payload(s)",
                self.errors
            )));
        }

        if self.unexpected_acks > 0 {
            return Err(Error::Custom(format!(
                "QuickNode soak received {} unexpected JSON-RPC ack(s)",
                self.unexpected_acks
            )));
        }

        if !self.pending_rpc_ids.is_empty() {
            return Err(Error::Custom(format!(
                "QuickNode soak finished with {} pending JSON-RPC request(s)",
                self.pending_rpc_ids.len()
            )));
        }

        if config
            .reconnect_every
            .is_some_and(|interval| interval.as_secs() <= config.soak_secs)
            && self.reconnects == 0
        {
            return Err(Error::Custom(
                "QuickNode soak reconnect interval was configured but no reconnect ran".to_string(),
            ));
        }

        if config
            .churn_every
            .is_some_and(|interval| interval.as_secs() <= config.soak_secs)
            && self.churns == 0
        {
            return Err(Error::Custom(
                "QuickNode soak churn interval was configured but no churn ran".to_string(),
            ));
        }

        if config.require_events && self.fills + self.account_events == 0 {
            return Err(Error::Custom(
                "QuickNode soak required at least one routed event but received none".to_string(),
            ));
        }

        if config.require_account_events && self.account_events == 0 {
            return Err(Error::Custom(
                "QuickNode soak required at least one account-event payload but received none"
                    .to_string(),
            ));
        }

        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    dotenv::dotenv().ok();

    if env::args().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        return Ok(());
    }

    let endpoint = quicknode_endpoint_from_env()?;
    let users = env::var("QN_SOAK_USERS")
        .map_err(|_| Error::Custom("QN_SOAK_USERS is required".to_string()))?
        .split(',')
        .map(|item| item.trim().parse::<Address>())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| Error::Custom(format!("invalid QN_SOAK_USERS address: {err}")))?;
    if users.is_empty() {
        return Err(Error::Custom("QN_SOAK_USERS must not be empty".to_string()));
    }

    let config = SoakConfig {
        soak_secs: parse_optional_seconds("QN_SOAK_SECONDS")?
            .map(|duration| duration.as_secs())
            .unwrap_or(DEFAULT_SOAK_SECS),
        reconnect_every: parse_optional_seconds("QN_SOAK_RECONNECT_EVERY_SECONDS")?,
        churn_every: parse_optional_seconds("QN_SOAK_CHURN_EVERY_SECONDS")?,
        require_events: parse_bool_env("QN_SOAK_REQUIRE_EVENTS")?,
        require_account_events: parse_bool_env("QN_SOAK_REQUIRE_ACCOUNT_EVENTS")?,
    };

    let ws_url = quicknode_ws_url(&endpoint)?;
    let user_filters = users
        .iter()
        .map(|address| format!("{address:#x}"))
        .collect::<Vec<_>>();

    println!(
        "connecting to QuickNode HyperCore websocket for {} user(s), duration={}s reconnect_every={:?} churn_every={:?} require_events={} require_account_events={}",
        user_filters.len(),
        config.soak_secs,
        config.reconnect_every,
        config.churn_every,
        config.require_events,
        config.require_account_events
    );

    let mut stats = SoakStats::default();
    let mut ws = connect_quicknode(&ws_url).await?;
    let mut rpc_id = 1;
    let mut generation = 0;
    let mut active_filters =
        send_subscriptions(&mut ws, &user_filters, generation, &mut rpc_id, &mut stats).await?;
    let deadline = Instant::now() + Duration::from_secs(config.soak_secs);
    let mut next_reconnect = config
        .reconnect_every
        .map(|duration| Instant::now() + duration);
    let mut next_churn = config.churn_every.map(|duration| Instant::now() + duration);
    while Instant::now() < deadline {
        let wait = next_wait(deadline, next_reconnect, next_churn);
        let mut connection_lost = None;
        if let Ok(message) = timeout(wait, ws.next()).await {
            match message {
                Some(Ok(Message::Text(text))) => classify_payload(&text, &mut stats),
                Some(Ok(Message::Binary(_))) => stats.non_json += 1,
                Some(Ok(Message::Ping(payload))) => {
                    if let Err(err) =
                        send_ws_message(&mut ws, Message::Pong(payload), "websocket pong").await
                    {
                        connection_lost = Some(format!("failed to send pong: {err}"));
                    }
                }
                Some(Ok(Message::Close(frame))) => {
                    connection_lost = Some(format!("close frame: {frame:?}"));
                }
                Some(Ok(Message::Pong(_) | Message::Frame(_))) => {}
                Some(Err(err)) => {
                    connection_lost = Some(format!("websocket error: {err}"));
                }
                None => {
                    connection_lost = Some("stream ended".to_string());
                }
            }
        }

        let now = Instant::now();
        if now >= deadline {
            break;
        }

        if let Some(reason) = connection_lost {
            eprintln!("QuickNode websocket disconnected during soak ({reason}); reconnecting");
            active_filters = reconnect_and_resubscribe(
                &ws_url,
                &mut ws,
                &user_filters,
                &mut generation,
                &mut rpc_id,
                &mut stats,
            )
            .await?;
            if let Some(interval) = config.reconnect_every {
                next_reconnect = Some(now + interval);
            }
            if let Some(interval) = config.churn_every {
                next_churn = Some(now + interval);
            }
            continue;
        }

        if next_reconnect.is_some_and(|due| now >= due) {
            active_filters = reconnect_and_resubscribe(
                &ws_url,
                &mut ws,
                &user_filters,
                &mut generation,
                &mut rpc_id,
                &mut stats,
            )
            .await?;
            if let Some(interval) = config.reconnect_every {
                next_reconnect = Some(now + interval);
            }
            if let Some(interval) = config.churn_every {
                next_churn = Some(now + interval);
            }
            continue;
        }

        if next_churn.is_some_and(|due| now >= due) {
            stats.churns = stats.churns.saturating_add(1);
            send_unsubscriptions(&mut ws, &active_filters, &mut rpc_id, &mut stats).await?;
            generation += 1;
            active_filters =
                send_subscriptions(&mut ws, &user_filters, generation, &mut rpc_id, &mut stats)
                    .await?;
            if let Some(interval) = config.churn_every {
                next_churn = Some(now + interval);
            }
        }
    }

    drain_pending_acks(&mut ws, &mut stats).await?;

    println!(
        "QuickNode soak complete: sent_requests={} acks={} unexpected_acks={} pending={} errors={} fills={} account_events={} unknown={} non_json={} reconnects={} churns={}",
        stats.sent_requests,
        stats.acks,
        stats.unexpected_acks,
        stats.pending_rpc_ids.len(),
        stats.errors,
        stats.fills,
        stats.account_events,
        stats.unknown,
        stats.non_json,
        stats.reconnects,
        stats.churns
    );

    stats.ensure_success(&config)?;

    Ok(())
}

async fn connect_quicknode(ws_url: &str) -> Result<Ws, Error> {
    match timeout(
        Duration::from_secs(QUICKNODE_CONNECT_TIMEOUT_SECS),
        connect_async(ws_url),
    )
    .await
    {
        Ok(Ok((ws, _))) => Ok(ws),
        Ok(Err(err)) => Err(Error::Custom(format!(
            "QuickNode websocket connect failed: {err}"
        ))),
        Err(_) => Err(Error::Custom(
            "QuickNode websocket connect timed out".to_string(),
        )),
    }
}

async fn reconnect_and_resubscribe(
    ws_url: &str,
    ws: &mut Ws,
    user_filters: &[String],
    generation: &mut u64,
    rpc_id: &mut u64,
    stats: &mut SoakStats,
) -> Result<ActiveFilters, Error> {
    let _ = ws.close(None).await;
    *ws = connect_quicknode(ws_url).await?;
    *generation = (*generation).saturating_add(1);
    stats.reconnects = stats.reconnects.saturating_add(1);
    stats.pending_rpc_ids.clear();
    send_subscriptions(ws, user_filters, *generation, rpc_id, stats).await
}

async fn drain_pending_acks(ws: &mut Ws, stats: &mut SoakStats) -> Result<(), Error> {
    let deadline = Instant::now() + Duration::from_secs(QUICKNODE_ACK_DRAIN_TIMEOUT_SECS);

    while !stats.pending_rpc_ids.is_empty() && Instant::now() < deadline {
        let wait = deadline.saturating_duration_since(Instant::now());
        match timeout(wait, ws.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => classify_payload(&text, stats),
            Ok(Some(Ok(Message::Binary(_)))) => stats.non_json += 1,
            Ok(Some(Ok(Message::Ping(payload)))) => {
                send_ws_message(ws, Message::Pong(payload), "websocket pong").await?;
            }
            Ok(Some(Ok(Message::Pong(_) | Message::Frame(_)))) => {}
            Ok(Some(Ok(Message::Close(_)))) | Ok(Some(Err(_))) | Ok(None) | Err(_) => break,
        }
    }

    Ok(())
}

fn quicknode_endpoint_from_env() -> Result<String, Error> {
    quicknode_endpoints_from_env()
        .into_iter()
        .next()
        .ok_or_else(|| {
            Error::Custom(
                "QUICKNODE_ENDPOINT, QUICKNODE_HYPERCORE_ENDPOINT, QUICKNODE_HYPERCORE_ENDPOINTS, or QUICKNODE_HYPERCORE_ENDPOINT1..10 is required".to_string(),
            )
        })
}

fn quicknode_endpoints_from_env() -> Vec<String> {
    let mut raw_values = Vec::with_capacity(QUICKNODE_NUMBERED_ENDPOINTS + 3);

    if let Ok(raw) = env::var("QUICKNODE_HYPERCORE_ENDPOINTS") {
        raw_values.push(raw);
    }

    for index in 1..=QUICKNODE_NUMBERED_ENDPOINTS {
        if let Ok(raw) = env::var(format!("QUICKNODE_HYPERCORE_ENDPOINT{index}")) {
            raw_values.push(raw);
        }
    }

    for key in ["QUICKNODE_HYPERCORE_ENDPOINT", "QUICKNODE_ENDPOINT"] {
        if let Ok(raw) = env::var(key) {
            raw_values.push(raw);
        }
    }

    split_quicknode_endpoint_values(raw_values)
}

fn split_quicknode_endpoint_values(raw_values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut endpoints = Vec::new();

    for endpoint in raw_values
        .into_iter()
        .flat_map(|raw| {
            raw.split(',')
                .map(str::trim)
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|endpoint| !endpoint.is_empty())
    {
        if seen.insert(endpoint.clone()) {
            endpoints.push(endpoint);
        }
    }

    endpoints
}

async fn send_subscriptions(
    ws: &mut Ws,
    user_filters: &[String],
    generation: u64,
    rpc_id: &mut u64,
    stats: &mut SoakStats,
) -> Result<ActiveFilters, Error> {
    let active_filters = ActiveFilters {
        fills: format!("soak_fills_{generation}"),
        account_events: format!("soak_account_events_{generation}"),
    };

    send_json(
        ws,
        json!({
            "jsonrpc": "2.0",
            "method": "hl_subscribe",
            "params": {
                "streamType": "trades",
                "filters": { "user": user_filters },
                "filterName": active_filters.fills.as_str()
            },
            "id": next_rpc_id(rpc_id, stats)
        }),
        "trades subscription",
    )
    .await?;

    send_json(
        ws,
        json!({
            "jsonrpc": "2.0",
            "method": "hl_subscribe",
            "params": {
                "streamType": "events",
                "filters": {
                    "users": user_filters,
                    "type": QUICKNODE_ACCOUNT_EVENT_TYPES
                },
                "filterName": active_filters.account_events.as_str()
            },
            "id": next_rpc_id(rpc_id, stats)
        }),
        "account events subscription",
    )
    .await?;

    Ok(active_filters)
}

async fn send_unsubscriptions(
    ws: &mut Ws,
    active_filters: &ActiveFilters,
    rpc_id: &mut u64,
    stats: &mut SoakStats,
) -> Result<(), Error> {
    for (filter_name, stream_type) in [
        (&active_filters.fills, "trades"),
        (&active_filters.account_events, "events"),
    ] {
        let id = next_rpc_id(rpc_id, stats);
        send_json(
            ws,
            unsubscribe_payload(filter_name, stream_type, id),
            "unsubscribe",
        )
        .await?;
    }

    Ok(())
}

fn unsubscribe_payload(filter_name: &str, stream_type: &str, id: u64) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "hl_unsubscribe",
        "params": {
            "streamType": stream_type,
            "filterName": filter_name
        },
        "id": id
    })
}

async fn send_json(ws: &mut Ws, payload: Value, label: &str) -> Result<(), Error> {
    send_ws_message(ws, Message::Text(payload.to_string().into()), label).await
}

async fn send_ws_message(ws: &mut Ws, message: Message, label: &str) -> Result<(), Error> {
    match timeout(
        Duration::from_secs(QUICKNODE_SEND_TIMEOUT_SECS),
        ws.send(message),
    )
    .await
    {
        Ok(Ok(())) => Ok(()),
        Ok(Err(err)) => Err(Error::Custom(format!("failed to send {label}: {err}"))),
        Err(_) => Err(Error::Custom(format!("timed out sending {label}"))),
    }
}

fn next_rpc_id(rpc_id: &mut u64, stats: &mut SoakStats) -> u64 {
    let id = *rpc_id;
    *rpc_id = (*rpc_id).saturating_add(1);
    stats.sent_requests = stats.sent_requests.saturating_add(1);
    stats.pending_rpc_ids.insert(id);
    id
}

fn next_wait(
    deadline: Instant,
    next_reconnect: Option<Instant>,
    next_churn: Option<Instant>,
) -> Duration {
    let now = Instant::now();
    let next_due = [Some(deadline), next_reconnect, next_churn]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(deadline);

    next_due.saturating_duration_since(now)
}

fn parse_optional_seconds(key: &str) -> Result<Option<Duration>, Error> {
    env::var(key)
        .ok()
        .map(|value| parse_optional_seconds_value(key, &value))
        .transpose()
        .map(Option::flatten)
}

fn parse_optional_seconds_value(key: &str, value: &str) -> Result<Option<Duration>, Error> {
    let seconds = value
        .parse::<u64>()
        .map_err(|err| Error::Custom(format!("invalid {key} value {value:?}: {err}")))?;

    Ok((seconds > 0).then(|| Duration::from_secs(seconds)))
}

fn parse_bool_env(key: &str) -> Result<bool, Error> {
    env::var(key)
        .ok()
        .map(|value| parse_bool_env_value(key, &value))
        .transpose()
        .map(|value| value.unwrap_or(false))
}

fn parse_bool_env_value(key: &str, value: &str) -> Result<bool, Error> {
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "0" | "false" | "no" | "off" => Ok(false),
        "1" | "true" | "yes" | "on" => Ok(true),
        _ => Err(Error::Custom(format!(
            "invalid {key} value {value:?}; expected true/false"
        ))),
    }
}

fn classify_payload(text: &str, stats: &mut SoakStats) {
    let Ok(payload) = serde_json::from_str::<Value>(text) else {
        stats.non_json += 1;
        return;
    };

    if payload.get("error").is_some() {
        stats.errors += 1;
        if let Some(id) = payload.get("id").and_then(Value::as_u64) {
            stats.pending_rpc_ids.remove(&id);
        }
        eprintln!("{payload}");
        return;
    }

    if payload.get("id").is_some() && payload.get("result").is_some() {
        if let Some(id) = payload.get("id").and_then(Value::as_u64) {
            if payload.get("result").and_then(Value::as_bool) == Some(false) {
                stats.errors += 1;
                stats.pending_rpc_ids.remove(&id);
                eprintln!("{payload}");
                return;
            }

            if stats.pending_rpc_ids.remove(&id) {
                stats.acks += 1;
            } else {
                stats.unexpected_acks += 1;
            }
        } else {
            stats.unexpected_acks += 1;
        }
        return;
    }

    match find_string_field(&payload, "filterName").as_deref() {
        Some(filter_name) if filter_name.starts_with("soak_fills") => stats.fills += 1,
        Some(filter_name) if filter_name.starts_with("soak_account_events") => {
            stats.account_events += 1;
        }
        _ => stats.unknown += 1,
    }
}

fn find_string_field(value: &Value, key: &str) -> Option<String> {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(found)) = map.get(key) {
                return Some(found.clone());
            }

            map.values()
                .find_map(|nested| find_string_field(nested, key))
        }
        Value::Array(items) => items.iter().find_map(|item| find_string_field(item, key)),
        _ => None,
    }
}

fn quicknode_ws_url(endpoint: &str) -> Result<String, Error> {
    let endpoint = endpoint.trim();
    if endpoint.is_empty() {
        return Err(Error::Custom("QUICKNODE_ENDPOINT is empty".to_string()));
    }

    let lower = endpoint.to_ascii_lowercase();
    let mut url = if lower.starts_with("https://") {
        format!("wss://{}", &endpoint["https://".len()..])
    } else if lower.starts_with("http://") {
        format!("ws://{}", &endpoint["http://".len()..])
    } else if lower.starts_with("wss://") || lower.starts_with("ws://") {
        endpoint.to_string()
    } else {
        return Err(Error::Custom(
            "QUICKNODE_ENDPOINT must start with http(s):// or ws(s)://".to_string(),
        ));
    };

    if !url.to_ascii_lowercase().contains(QUICKNODE_WS_PATH) {
        url = format!("{}{}", url.trim_end_matches('/'), QUICKNODE_WS_PATH);
    }

    Ok(url)
}

fn print_help() {
    println!(
        "QuickNode HyperCore soak test\n\n\
         Required env:\n\
           QUICKNODE_ENDPOINT=https://...\n\
             or QUICKNODE_HYPERCORE_ENDPOINT=https://...\n\
             or QUICKNODE_HYPERCORE_ENDPOINTS=https://...,...\n\
             or QUICKNODE_HYPERCORE_ENDPOINT1=https://... through QUICKNODE_HYPERCORE_ENDPOINT10=https://...\n\
           QN_SOAK_USERS=0xabc...,0xdef...\n\n\
         Optional env:\n\
           QN_SOAK_SECONDS=60\n\n\
           QN_SOAK_RECONNECT_EVERY_SECONDS=0\n\
           QN_SOAK_CHURN_EVERY_SECONDS=0\n\
           QN_SOAK_REQUIRE_EVENTS=false\n\
           QN_SOAK_REQUIRE_ACCOUNT_EVENTS=false\n\n\
         Example:\n\
           QUICKNODE_ENDPOINT=$ENDPOINT QN_SOAK_USERS=$ADDRESS cargo run --release --bin qn_soak"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quicknode_ws_url_converts_http_and_appends_path() {
        let url = quicknode_ws_url("https://example.quiknode.pro/token").unwrap();
        assert_eq!(
            url,
            "wss://example.quiknode.pro/token/hypercore/ws".to_string()
        );
    }

    #[test]
    fn quicknode_ws_url_keeps_existing_ws_path() {
        let url = quicknode_ws_url("wss://example.quiknode.pro/token/hypercore/ws").unwrap();
        assert_eq!(url, "wss://example.quiknode.pro/token/hypercore/ws");
    }

    #[test]
    fn split_quicknode_endpoint_values_keeps_numbered_and_comma_sources() {
        let endpoints = split_quicknode_endpoint_values([
            "https://endpoint-1.quicknode.example, https://endpoint-2.quicknode.example"
                .to_string(),
            "https://endpoint-3.quicknode.example".to_string(),
            "https://endpoint-2.quicknode.example".to_string(),
            "  ".to_string(),
        ]);

        assert_eq!(
            endpoints,
            vec![
                "https://endpoint-1.quicknode.example",
                "https://endpoint-2.quicknode.example",
                "https://endpoint-3.quicknode.example",
            ]
        );
    }

    #[test]
    fn classify_payload_counts_ack_and_routes() {
        let mut stats = SoakStats::default();
        stats.pending_rpc_ids.insert(1);

        classify_payload(r#"{"jsonrpc":"2.0","id":1,"result":true}"#, &mut stats);
        classify_payload(r#"{"filterName":"soak_fills_7","data":[]}"#, &mut stats);
        classify_payload(
            r#"{"nested":{"filterName":"soak_account_events_7"}}"#,
            &mut stats,
        );
        classify_payload(r#"{"error":{"message":"boom"}}"#, &mut stats);
        classify_payload("not json", &mut stats);

        assert_eq!(stats.acks, 1);
        assert_eq!(stats.unexpected_acks, 0);
        assert_eq!(stats.fills, 1);
        assert_eq!(stats.account_events, 1);
        assert_eq!(stats.errors, 1);
        assert_eq!(stats.non_json, 1);
    }

    #[test]
    fn classify_payload_treats_false_result_as_provider_error() {
        let mut stats = SoakStats::default();
        stats.pending_rpc_ids.insert(7);

        classify_payload(r#"{"jsonrpc":"2.0","id":7,"result":false}"#, &mut stats);

        assert_eq!(stats.acks, 0);
        assert_eq!(stats.errors, 1);
        assert!(stats.pending_rpc_ids.is_empty());
    }

    #[test]
    fn unsubscribe_payload_includes_stream_type_for_provider_contract() {
        let payload = unsubscribe_payload("soak_fills_1", "trades", 42);

        assert_eq!(payload["method"], json!("hl_unsubscribe"));
        assert_eq!(payload["params"]["filterName"], json!("soak_fills_1"));
        assert_eq!(payload["params"]["streamType"], json!("trades"));
        assert_eq!(payload["id"], json!(42));
    }

    #[test]
    fn parse_optional_seconds_value_treats_zero_as_disabled() {
        assert_eq!(
            parse_optional_seconds_value("TEST", "0")
                .expect("zero should parse")
                .map(|duration| duration.as_secs()),
            None
        );
        assert_eq!(
            parse_optional_seconds_value("TEST", "15")
                .expect("seconds should parse")
                .map(|duration| duration.as_secs()),
            Some(15)
        );
        assert!(parse_optional_seconds_value("TEST", "bad").is_err());
    }

    #[test]
    fn parse_bool_env_value_accepts_common_forms() {
        assert!(parse_bool_env_value("TEST", "true").expect("true should parse"));
        assert!(parse_bool_env_value("TEST", "1").expect("1 should parse"));
        assert!(!parse_bool_env_value("TEST", "false").expect("false should parse"));
        assert!(!parse_bool_env_value("TEST", "0").expect("0 should parse"));
        assert!(parse_bool_env_value("TEST", "maybe").is_err());
    }

    #[test]
    fn soak_stats_fail_on_provider_errors_or_missing_configured_actions() {
        let config = SoakConfig {
            soak_secs: 60,
            reconnect_every: Some(Duration::from_secs(10)),
            churn_every: Some(Duration::from_secs(10)),
            require_events: false,
            require_account_events: false,
        };
        let mut stats = SoakStats {
            sent_requests: 2,
            acks: 2,
            reconnects: 1,
            churns: 1,
            ..SoakStats::default()
        };
        assert!(stats.ensure_success(&config).is_ok());

        stats.errors = 1;
        assert!(stats.ensure_success(&config).is_err());

        stats.errors = 0;
        stats.reconnects = 0;
        assert!(stats.ensure_success(&config).is_err());

        stats.reconnects = 1;
        stats.churns = 0;
        assert!(stats.ensure_success(&config).is_err());

        stats.churns = 1;
        stats.pending_rpc_ids.insert(2);
        assert!(stats.ensure_success(&config).is_err());

        stats.pending_rpc_ids.clear();
        stats.unexpected_acks = 1;
        assert!(stats.ensure_success(&config).is_err());

        let require_events = SoakConfig {
            require_events: true,
            ..config
        };
        let mut stats = SoakStats {
            sent_requests: 2,
            acks: 2,
            reconnects: 1,
            churns: 1,
            ..SoakStats::default()
        };
        assert!(stats.ensure_success(&require_events).is_err());
        stats.fills = 1;
        assert!(stats.ensure_success(&require_events).is_ok());

        let require_account_events = SoakConfig {
            require_account_events: true,
            ..config
        };
        assert!(stats.ensure_success(&require_account_events).is_err());
        stats.account_events = 1;
        assert!(stats.ensure_success(&require_account_events).is_ok());
    }
}
