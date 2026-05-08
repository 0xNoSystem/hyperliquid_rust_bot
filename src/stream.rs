#![allow(dead_code)]

use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use alloy::primitives::Address;
use futures_util::{SinkExt, StreamExt, stream::SplitSink};
use log::{error, info, warn};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use tokio::{
    net::TcpStream,
    spawn,
    sync::{Mutex, mpsc::UnboundedSender},
    time,
};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::protocol};

use crate::{BaseUrl, Error, HLTradeInfo};
use hyperliquid_rust_sdk::UserFunding;

type Result<T> = std::result::Result<T, Error>;
type WsWriter = SplitSink<WebSocketStream<MaybeTlsStream<TcpStream>>, protocol::Message>;

const QUICKNODE_WS_PATH: &str = "/hypercore/ws";

#[derive(Clone, Debug)]
pub(crate) enum AccountEvent {
    Fill(Vec<AccountFill>),
    Funding(Vec<AccountFunding>),
    Raw {
        stream_type: QuickNodeStreamType,
        payload: Value,
    },
    Error(String),
    NoData,
}

#[derive(Clone, Debug)]
pub(crate) struct AccountFill {
    pub(crate) user: Address,
    pub(crate) fill: HLTradeInfo,
    pub(crate) block: QuickNodeBlockMeta,
}

#[derive(Clone, Debug)]
pub(crate) struct AccountFunding {
    pub(crate) user: Address,
    pub(crate) funding: UserFunding,
    pub(crate) block: QuickNodeBlockMeta,
}

#[derive(Clone, Debug)]
pub(crate) struct QuickNodeBlockMeta {
    pub(crate) block_number: u64,
    pub(crate) block_time: String,
    pub(crate) local_time: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum QuickNodeStreamType {
    Trades,
    Orders,
    BookUpdates,
    TwapOrders,
    Events,
    WriterActions,
}

impl QuickNodeStreamType {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Trades => "trades",
            Self::Orders => "orders",
            Self::BookUpdates => "book_updates",
            Self::TwapOrders => "twap_orders",
            Self::Events => "events",
            Self::WriterActions => "writer_actions",
        }
    }

    fn from_str(stream_type: &str) -> Option<Self> {
        match stream_type {
            "trades" | "hl.trades" => Some(Self::Trades),
            "orders" | "hl.orders" => Some(Self::Orders),
            "book_updates" | "hl.book_updates" => Some(Self::BookUpdates),
            "twap_orders" | "hl.twap_orders" => Some(Self::TwapOrders),
            "events" | "hl.events" => Some(Self::Events),
            "writer_actions" | "hl.writer_actions" => Some(Self::WriterActions),
            _ => None,
        }
    }
}

#[derive(Deserialize)]
struct QuickNodeEnvelope<T> {
    block: QuickNodeBlock<T>,
}

#[derive(Deserialize)]
struct QuickNodeBlock<T> {
    block_number: u64,
    block_time: String,
    local_time: String,
    events: Vec<(Address, T)>,
}

impl<T> QuickNodeBlock<T> {
    fn meta(&self) -> QuickNodeBlockMeta {
        QuickNodeBlockMeta {
            block_number: self.block_number,
            block_time: self.block_time.clone(),
            local_time: self.local_time.clone(),
        }
    }
}

#[derive(Clone, Debug)]
struct QnSubscription {
    route_key: String,
    stream_type: QuickNodeStreamType,
    subscribe_payload: Value,
    unsubscribe_payload: Value,
}

#[derive(Clone, Debug)]
struct RouteSubscriber {
    sending_channel: UnboundedSender<AccountEvent>,
    subscription_id: u32,
    qn_subscription: QnSubscription,
}

#[derive(Debug)]
struct AccountDelivery {
    stream_type: QuickNodeStreamType,
    user_filters: HashSet<String>,
    senders: Vec<UnboundedSender<AccountEvent>>,
}

#[derive(Clone, Copy, Debug)]
struct LivenessConfig {
    ping_after: Duration,
    pong_grace: Duration,
    check_interval: Duration,
}

pub(crate) struct EventStream {
    stop_flag: Arc<AtomicBool>,
    writer: Arc<Mutex<WsWriter>>,
    subscriptions: Arc<Mutex<HashMap<String, Vec<RouteSubscriber>>>>,
    subscription_routes: HashMap<u32, Vec<String>>,
    subscription_id: u32,
    jsonrpc_id: Arc<AtomicU64>,
}

impl EventStream {
    const MAINNET_PING_AFTER_SECS: u64 = 20;
    const MAINNET_PONG_GRACE_SECS: u64 = 22;
    const TESTNET_PING_AFTER_SECS: u64 = 40;
    const TESTNET_PONG_GRACE_SECS: u64 = 45;
    const LIVENESS_CHECK_INTERVAL_SECS: u64 = 10;

    pub(crate) async fn new(
        endpoint: String,
        reconnect: bool,
        base_url: BaseUrl,
    ) -> Result<EventStream> {
        if !is_quicknode_endpoint(&endpoint) {
            return Err(Error::Custom("Invalid QuickNode endpoint".to_string()));
        }

        let url = build_ws_url(&endpoint);
        let stop_flag = Arc::new(AtomicBool::new(false));
        let base_instant = Instant::now();
        let last_rx = Arc::new(AtomicU64::new(0));
        let last_pong = Arc::new(AtomicU64::new(0));
        let last_ping = Arc::new(AtomicU64::new(0));
        let awaiting_pong = Arc::new(AtomicBool::new(false));
        let force_reconnect = Arc::new(AtomicBool::new(false));
        let jsonrpc_id = Arc::new(AtomicU64::new(1));

        let (writer, mut reader) = Self::connect(&url).await?.split();
        let writer = Arc::new(Mutex::new(writer));
        let subscriptions = Arc::new(Mutex::new(HashMap::new()));
        let liveness = Self::liveness_config(base_url);

        {
            let url = url.clone();
            let writer = Arc::clone(&writer);
            let subscriptions = Arc::clone(&subscriptions);
            let stop_flag = Arc::clone(&stop_flag);
            let last_rx = Arc::clone(&last_rx);
            let last_pong = Arc::clone(&last_pong);
            let last_ping = Arc::clone(&last_ping);
            let awaiting_pong = Arc::clone(&awaiting_pong);
            let force_reconnect = Arc::clone(&force_reconnect);
            let jsonrpc_id = Arc::clone(&jsonrpc_id);

            spawn(async move {
                while !stop_flag.load(Ordering::Relaxed) {
                    let mut should_reconnect = false;
                    let next = if let Some(cfg) = liveness {
                        time::timeout(cfg.check_interval, reader.next()).await
                    } else {
                        Ok(reader.next().await)
                    };
                    let now_ms = base_instant.elapsed().as_millis() as u64;

                    match next {
                        Ok(Some(Ok(data))) => {
                            last_rx.store(now_ms, Ordering::Relaxed);
                            if awaiting_pong.load(Ordering::Relaxed) {
                                awaiting_pong.store(false, Ordering::Relaxed);
                            }

                            match data {
                                protocol::Message::Text(text) => {
                                    if let Err(err) =
                                        parse_and_send_data(text.to_string(), &subscriptions).await
                                    {
                                        error!("Error processing QuickNode websocket data: {err}");
                                        should_reconnect = true;
                                    }
                                }
                                protocol::Message::Close(frame) => {
                                    warn!("QuickNode websocket received close frame: {frame:?}");
                                    should_reconnect = true;
                                }
                                protocol::Message::Pong(_) => {
                                    last_pong.store(now_ms, Ordering::Relaxed);
                                    awaiting_pong.store(false, Ordering::Relaxed);
                                }
                                protocol::Message::Ping(data) => {
                                    let mut writer = writer.lock().await;
                                    if let Err(err) =
                                        writer.send(protocol::Message::Pong(data)).await
                                    {
                                        error!("Error replying to QuickNode websocket ping: {err}");
                                        should_reconnect = true;
                                    }
                                }
                                protocol::Message::Binary(_) => {}
                                _ => {}
                            }
                        }
                        Ok(Some(Err(err))) => {
                            error!("QuickNode websocket reader error: {err}");
                            send_to_all_subscriptions(
                                &subscriptions,
                                AccountEvent::Error(
                                    Error::GenericReader(err.to_string()).to_string(),
                                ),
                            )
                            .await;
                            should_reconnect = true;
                        }
                        Ok(None) => {
                            warn!("QuickNode websocket disconnected");
                            should_reconnect = true;
                        }
                        Err(_) => {
                            if let Some(cfg) = liveness {
                                let last_ping_ms = last_ping.load(Ordering::Relaxed);
                                let last_rx_ms = last_rx.load(Ordering::Relaxed);
                                let last_pong_ms = last_pong.load(Ordering::Relaxed);
                                if awaiting_pong.load(Ordering::Relaxed)
                                    && now_ms.saturating_sub(last_ping_ms)
                                        >= cfg.pong_grace.as_millis() as u64
                                    && last_rx_ms <= last_ping_ms
                                    && last_pong_ms <= last_ping_ms
                                {
                                    warn!("QuickNode websocket pong timeout");
                                    should_reconnect = true;
                                }
                            }
                        }
                    }

                    if !should_reconnect && force_reconnect.swap(false, Ordering::Relaxed) {
                        should_reconnect = true;
                    }

                    if should_reconnect {
                        send_to_all_subscriptions(&subscriptions, AccountEvent::NoData).await;

                        if reconnect {
                            loop {
                                if stop_flag.load(Ordering::Relaxed) {
                                    break;
                                }

                                time::sleep(Duration::from_secs(1)).await;
                                info!("QuickNode websocket attempting to reconnect");

                                match Self::connect(&url).await {
                                    Ok(ws) => {
                                        let (new_writer, new_reader) = ws.split();
                                        reader = new_reader;

                                        {
                                            let mut writer_guard = writer.lock().await;
                                            *writer_guard = new_writer;
                                        }

                                        awaiting_pong.store(false, Ordering::Relaxed);
                                        last_rx.store(
                                            base_instant.elapsed().as_millis() as u64,
                                            Ordering::Relaxed,
                                        );

                                        if let Err(err) = replay_subscriptions(
                                            &writer,
                                            &subscriptions,
                                            &jsonrpc_id,
                                        )
                                        .await
                                        {
                                            error!(
                                                "Error replaying QuickNode subscriptions after reconnect: {err}"
                                            );
                                            force_reconnect.store(true, Ordering::Relaxed);
                                        } else {
                                            info!("QuickNode websocket reconnect finished");
                                        }

                                        break;
                                    }
                                    Err(err) => {
                                        error!("Could not reconnect to QuickNode websocket: {err}");
                                    }
                                }
                            }
                        } else {
                            error!(
                                "QuickNode websocket reconnection disabled; reader task exiting"
                            );
                            break;
                        }
                    }
                }

                warn!("QuickNode websocket reader task stopped");
            });
        }

        if let Some(liveness) = liveness {
            let writer = Arc::clone(&writer);
            let stop_flag = Arc::clone(&stop_flag);
            let last_rx = Arc::clone(&last_rx);
            let last_ping = Arc::clone(&last_ping);
            let awaiting_pong = Arc::clone(&awaiting_pong);
            let force_reconnect = Arc::clone(&force_reconnect);

            spawn(async move {
                while !stop_flag.load(Ordering::Relaxed) {
                    let now_ms = base_instant.elapsed().as_millis() as u64;
                    let last_rx_ms = last_rx.load(Ordering::Relaxed);

                    if !awaiting_pong.load(Ordering::Relaxed)
                        && now_ms.saturating_sub(last_rx_ms)
                            >= liveness.ping_after.as_millis() as u64
                    {
                        let mut writer = writer.lock().await;
                        match writer
                            .send(protocol::Message::Ping(Vec::new().into()))
                            .await
                        {
                            Ok(()) => {
                                awaiting_pong.store(true, Ordering::Relaxed);
                                last_ping.store(now_ms, Ordering::Relaxed);
                            }
                            Err(err) => {
                                error!("Error pinging QuickNode websocket: {err}");
                                force_reconnect.store(true, Ordering::Relaxed);
                            }
                        }
                    }

                    time::sleep(liveness.check_interval).await;
                }

                warn!("QuickNode websocket ping task stopped");
            });
        }

        Ok(EventStream {
            stop_flag,
            writer,
            subscriptions,
            subscription_routes: HashMap::new(),
            subscription_id: 1,
            jsonrpc_id,
        })
    }

    pub(crate) async fn subscribe_user_events(
        &mut self,
        users: Vec<Address>,
        sending_channel: UnboundedSender<AccountEvent>,
    ) -> Result<u32> {
        let subscription_id = self.subscription_id;
        self.subscription_id = self
            .subscription_id
            .checked_add(1)
            .ok_or_else(|| Error::Custom("Subscription id overflow".to_string()))?;

        let users = users
            .iter()
            .map(|address| format!("{address:#x}"))
            .collect::<Vec<_>>();

        let fills_route = format!("fills_{subscription_id}");
        let fundings_route = format!("fundings_{subscription_id}");
        let qn_subscriptions = vec![
            build_qn_subscription(
                fills_route.clone(),
                QuickNodeStreamType::Trades,
                json!({ "user": users.clone() }),
                next_rpc_id(&self.jsonrpc_id),
                next_rpc_id(&self.jsonrpc_id),
            ),
            build_qn_subscription(
                fundings_route.clone(),
                QuickNodeStreamType::Events,
                json!({
                    "users": users,
                    "type": ["funding"],
                }),
                next_rpc_id(&self.jsonrpc_id),
                next_rpc_id(&self.jsonrpc_id),
            ),
        ];

        self.subscription_routes
            .insert(subscription_id, vec![fills_route, fundings_route]);

        for qn_subscription in qn_subscriptions {
            if let Err(err) = self
                .add_route_subscription(qn_subscription, sending_channel.clone(), subscription_id)
                .await
            {
                let _ = self.remove_subscription(subscription_id).await;
                return Err(err);
            }
        }

        Ok(subscription_id)
    }

    pub(crate) async fn remove_subscription(&mut self, subscription_id: u32) -> Result<()> {
        let route_keys = self
            .subscription_routes
            .remove(&subscription_id)
            .ok_or(Error::SubscriptionNotFound)?;
        let mut unsubscribe_payloads = Vec::new();

        {
            let mut subscriptions = self.subscriptions.lock().await;

            for route_key in route_keys {
                let mut should_remove_route = false;

                if let Some(route_subscribers) = subscriptions.get_mut(&route_key) {
                    let qn_subscription = route_subscribers
                        .first()
                        .map(|subscriber| subscriber.qn_subscription.clone());
                    route_subscribers
                        .retain(|subscriber| subscriber.subscription_id != subscription_id);

                    if route_subscribers.is_empty() {
                        should_remove_route = true;
                        if let Some(qn_subscription) = qn_subscription {
                            unsubscribe_payloads.push(payload_with_rpc_id(
                                &qn_subscription.unsubscribe_payload,
                                next_rpc_id(&self.jsonrpc_id),
                            ));
                        }
                    }
                }

                if should_remove_route {
                    subscriptions.remove(&route_key);
                }
            }
        }

        send_payloads(&self.writer, unsubscribe_payloads).await
    }

    pub(crate) async fn unsubscribe_all(&mut self) -> Result<()> {
        let unsubscribe_payloads = {
            let mut subscriptions = self.subscriptions.lock().await;
            let payloads = subscriptions
                .values()
                .filter_map(|route_subscribers| {
                    route_subscribers.first().map(|subscriber| {
                        payload_with_rpc_id(
                            &subscriber.qn_subscription.unsubscribe_payload,
                            next_rpc_id(&self.jsonrpc_id),
                        )
                    })
                })
                .collect::<Vec<_>>();

            subscriptions.clear();
            payloads
        };

        self.subscription_routes.clear();
        send_payloads(&self.writer, unsubscribe_payloads).await
    }

    async fn connect(url: &str) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>> {
        Ok(connect_async(url)
            .await
            .map_err(|err| Error::Websocket(err.to_string()))?
            .0)
    }

    fn liveness_config(base_url: BaseUrl) -> Option<LivenessConfig> {
        match base_url {
            BaseUrl::Testnet => Some(LivenessConfig {
                ping_after: Duration::from_secs(Self::TESTNET_PING_AFTER_SECS),
                pong_grace: Duration::from_secs(Self::TESTNET_PONG_GRACE_SECS),
                check_interval: Duration::from_secs(Self::LIVENESS_CHECK_INTERVAL_SECS),
            }),
            BaseUrl::Mainnet => Some(LivenessConfig {
                ping_after: Duration::from_secs(Self::MAINNET_PING_AFTER_SECS),
                pong_grace: Duration::from_secs(Self::MAINNET_PONG_GRACE_SECS),
                check_interval: Duration::from_secs(Self::LIVENESS_CHECK_INTERVAL_SECS),
            }),
            BaseUrl::Localhost => None,
        }
    }

    async fn add_route_subscription(
        &self,
        qn_subscription: QnSubscription,
        sending_channel: UnboundedSender<AccountEvent>,
        subscription_id: u32,
    ) -> Result<()> {
        let should_subscribe = {
            let subscriptions = self.subscriptions.lock().await;
            subscriptions
                .get(&qn_subscription.route_key)
                .is_none_or(Vec::is_empty)
        };

        if should_subscribe {
            send_payloads(
                &self.writer,
                vec![qn_subscription.subscribe_payload.clone()],
            )
            .await?;
        }

        let mut subscriptions = self.subscriptions.lock().await;
        subscriptions
            .entry(qn_subscription.route_key.clone())
            .or_insert_with(Vec::new)
            .push(RouteSubscriber {
                sending_channel,
                subscription_id,
                qn_subscription,
            });

        Ok(())
    }
}

impl Drop for EventStream {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
    }
}

async fn parse_and_send_data(
    data: String,
    subscriptions: &Arc<Mutex<HashMap<String, Vec<RouteSubscriber>>>>,
) -> Result<()> {
    let Ok(payload) = serde_json::from_str::<Value>(&data) else {
        return Ok(());
    };

    if !payload.is_object() {
        return Ok(());
    }

    if let Some(error) = payload.get("error") {
        send_error_event(&payload, error_to_string(error), subscriptions).await;
        return Ok(());
    }

    if payload.get("id").is_some() && payload.get("result").is_some() {
        return Ok(());
    }

    route_account_event(payload, subscriptions).await;
    Ok(())
}

async fn route_account_event(
    payload: Value,
    subscriptions: &Arc<Mutex<HashMap<String, Vec<RouteSubscriber>>>>,
) {
    let filter_name = find_string_field(&payload, "filterName");
    let stream_name = find_string_field(&payload, "streamType")
        .or_else(|| find_string_field(&payload, "stream"))
        .or_else(|| find_string_field(&payload, "channel"));
    let payload_users = quicknode_event_users(&payload);

    let deliveries = {
        let subscriptions = subscriptions.lock().await;

        if let Some(filter_name) = filter_name {
            if let Some(route_subscribers) = subscriptions.get(&filter_name) {
                route_deliveries(route_subscribers)
            } else {
                warn!("Dropping QuickNode message for unknown filterName={filter_name}");
                Vec::new()
            }
        } else if let Some(stream_name) = stream_name {
            if let Some(stream_type) = QuickNodeStreamType::from_str(&stream_name) {
                subscriptions
                    .values()
                    .filter(|route_subscribers| {
                        route_subscribers.first().is_some_and(|subscriber| {
                            subscriber.qn_subscription.stream_type == stream_type
                        })
                    })
                    .filter(|route_subscribers| {
                        route_matches_payload_users(route_subscribers, &payload_users)
                    })
                    .flat_map(|route_subscribers| route_deliveries(route_subscribers))
                    .collect::<Vec<_>>()
            } else {
                warn!("Dropping QuickNode message for unknown streamType={stream_name}");
                Vec::new()
            }
        } else {
            warn!("Dropping QuickNode message without route fields");
            Vec::new()
        }
    };

    for delivery in deliveries {
        if let Some(event) = account_event_for_stream(
            delivery.stream_type,
            &delivery.user_filters,
            payload.clone(),
        ) {
            for sender in delivery.senders {
                send_account_event(&sender, event.clone());
            }
        }
    }
}

fn route_deliveries(route_subscribers: &[RouteSubscriber]) -> Vec<AccountDelivery> {
    let Some(first) = route_subscribers.first() else {
        return Vec::new();
    };

    let mut seen = HashSet::new();
    let senders = route_subscribers
        .iter()
        .filter_map(|subscriber| {
            seen.insert(subscriber.subscription_id)
                .then(|| subscriber.sending_channel.clone())
        })
        .collect::<Vec<_>>();

    vec![AccountDelivery {
        stream_type: first.qn_subscription.stream_type,
        user_filters: subscription_user_filters(&first.qn_subscription),
        senders,
    }]
}

fn route_matches_payload_users(
    route_subscribers: &[RouteSubscriber],
    payload_users: &HashSet<String>,
) -> bool {
    if payload_users.is_empty() {
        return true;
    }

    route_subscribers
        .first()
        .map(|subscriber| {
            let route_users = subscription_user_filters(&subscriber.qn_subscription);
            route_users.is_empty() || route_users.iter().any(|user| payload_users.contains(user))
        })
        .unwrap_or(false)
}

async fn send_error_event(
    payload: &Value,
    error: String,
    subscriptions: &Arc<Mutex<HashMap<String, Vec<RouteSubscriber>>>>,
) {
    if let Some(filter_name) = find_string_field(payload, "filterName") {
        let senders = {
            let subscriptions = subscriptions.lock().await;
            subscriptions
                .get(&filter_name)
                .map(|route_subscribers| {
                    let mut seen = HashSet::new();
                    route_subscribers
                        .iter()
                        .filter_map(|subscriber| {
                            seen.insert(subscriber.subscription_id)
                                .then(|| subscriber.sending_channel.clone())
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        };

        if !senders.is_empty() {
            for sender in senders {
                send_account_event(&sender, AccountEvent::Error(error.clone()));
            }
            return;
        }
    }

    send_to_all_subscriptions(subscriptions, AccountEvent::Error(error)).await;
}

async fn send_to_all_subscriptions(
    subscriptions: &Arc<Mutex<HashMap<String, Vec<RouteSubscriber>>>>,
    event: AccountEvent,
) {
    let senders = {
        let subscriptions = subscriptions.lock().await;
        let mut seen = HashSet::new();
        let mut senders = Vec::new();

        for route_subscribers in subscriptions.values() {
            for subscriber in route_subscribers {
                if seen.insert(subscriber.subscription_id) {
                    senders.push(subscriber.sending_channel.clone());
                }
            }
        }

        senders
    };

    for sender in senders {
        send_account_event(&sender, event.clone());
    }
}

fn send_account_event(sender: &UnboundedSender<AccountEvent>, event: AccountEvent) {
    if let Err(err) = sender.send(event) {
        warn!("Error sending account event from QuickNode stream: {err}");
    }
}

async fn replay_subscriptions(
    writer: &Arc<Mutex<WsWriter>>,
    subscriptions: &Arc<Mutex<HashMap<String, Vec<RouteSubscriber>>>>,
    jsonrpc_id: &Arc<AtomicU64>,
) -> Result<()> {
    let payloads = {
        let subscriptions = subscriptions.lock().await;
        subscriptions
            .values()
            .filter_map(|route_subscribers| {
                route_subscribers.first().map(|subscriber| {
                    payload_with_rpc_id(
                        &subscriber.qn_subscription.subscribe_payload,
                        next_rpc_id(jsonrpc_id),
                    )
                })
            })
            .collect::<Vec<_>>()
    };

    send_payloads(writer, payloads).await
}

async fn send_payloads(writer: &Arc<Mutex<WsWriter>>, payloads: Vec<Value>) -> Result<()> {
    if payloads.is_empty() {
        return Ok(());
    }

    let mut writer = writer.lock().await;
    let mut result = Ok(());

    for payload in payloads {
        if let Err(err) = writer
            .send(protocol::Message::Text(payload.to_string().into()))
            .await
            .map_err(|err| Error::WsSend(err.to_string()))
        {
            result = Err(err);
        }
    }

    result
}

fn build_qn_subscription(
    route_key: String,
    stream_type: QuickNodeStreamType,
    filters: Value,
    subscribe_id: u64,
    unsubscribe_id: u64,
) -> QnSubscription {
    QnSubscription {
        subscribe_payload: json!({
            "jsonrpc": "2.0",
            "method": "hl_subscribe",
            "params": {
                "streamType": stream_type.as_str(),
                "filters": filters,
                "filterName": route_key.clone(),
            },
            "id": subscribe_id,
        }),
        unsubscribe_payload: json!({
            "jsonrpc": "2.0",
            "method": "hl_unsubscribe",
            "params": {
                "filterName": route_key.clone(),
            },
            "id": unsubscribe_id,
        }),
        route_key,
        stream_type,
    }
}

fn payload_with_rpc_id(payload: &Value, id: u64) -> Value {
    let mut payload = payload.clone();
    if let Value::Object(map) = &mut payload {
        map.insert("id".to_string(), json!(id));
    }
    payload
}

fn account_event_for_stream(
    stream_type: QuickNodeStreamType,
    user_filters: &HashSet<String>,
    payload: Value,
) -> Option<AccountEvent> {
    match stream_type {
        QuickNodeStreamType::Trades => match parse_account_fills(&payload, user_filters) {
            Ok(fills) if fills.is_empty() => None,
            Ok(fills) => Some(AccountEvent::Fill(fills)),
            Err(err) => {
                warn!("Falling back to raw QuickNode trades payload: {err}");
                Some(AccountEvent::Raw {
                    stream_type,
                    payload,
                })
            }
        },
        QuickNodeStreamType::Events => match parse_account_fundings(&payload, user_filters) {
            Ok(fundings) if fundings.is_empty() => None,
            Ok(fundings) => Some(AccountEvent::Funding(fundings)),
            Err(err) => {
                warn!("Falling back to raw QuickNode events payload: {err}");
                Some(AccountEvent::Raw {
                    stream_type,
                    payload,
                })
            }
        },
        _ => Some(AccountEvent::Raw {
            stream_type,
            payload,
        }),
    }
}

fn parse_account_fills(
    payload: &Value,
    user_filters: &HashSet<String>,
) -> std::result::Result<Vec<AccountFill>, serde_json::Error> {
    Ok(parse_quicknode_account_events::<HLTradeInfo>(payload)?
        .into_iter()
        .filter(|(user, _, _)| user_matches_filters(user, user_filters))
        .map(|(user, fill, block)| AccountFill { user, fill, block })
        .collect())
}

fn parse_account_fundings(
    payload: &Value,
    user_filters: &HashSet<String>,
) -> std::result::Result<Vec<AccountFunding>, serde_json::Error> {
    Ok(parse_quicknode_account_events::<UserFunding>(payload)?
        .into_iter()
        .filter(|(user, _, _)| user_matches_filters(user, user_filters))
        .map(|(user, funding, block)| AccountFunding {
            user,
            funding,
            block,
        })
        .collect())
}

fn parse_quicknode_account_events<T>(
    payload: &Value,
) -> std::result::Result<Vec<(Address, T, QuickNodeBlockMeta)>, serde_json::Error>
where
    T: DeserializeOwned,
{
    let envelope = serde_json::from_value::<QuickNodeEnvelope<T>>(payload.clone())?;
    let meta = envelope.block.meta();

    Ok(envelope
        .block
        .events
        .into_iter()
        .map(|(user, event)| (user, event, meta.clone()))
        .collect())
}

fn user_matches_filters(user: &Address, user_filters: &HashSet<String>) -> bool {
    user_filters.is_empty() || user_filters.contains(&normalize_user(&format!("{user:#x}")))
}

fn quicknode_event_users(payload: &Value) -> HashSet<String> {
    payload
        .get("block")
        .and_then(|block| block.get("events"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|event| event.as_array())
        .filter_map(|event| event.first())
        .filter_map(Value::as_str)
        .map(normalize_user)
        .collect()
}

fn subscription_user_filters(subscription: &QnSubscription) -> HashSet<String> {
    let filters = subscription
        .subscribe_payload
        .get("params")
        .and_then(|params| params.get("filters"));

    ["user", "users"]
        .into_iter()
        .filter_map(|field| filters.and_then(|filters| filters.get(field)))
        .flat_map(value_strings)
        .map(normalize_user)
        .collect()
}

fn value_strings(value: &Value) -> Vec<&str> {
    match value {
        Value::String(value) => vec![value.as_str()],
        Value::Array(values) => values.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    }
}

fn normalize_user(user: &str) -> String {
    user.to_ascii_lowercase()
}

fn find_string_field(value: &Value, field: &str) -> Option<String> {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(value)) = map.get(field) {
                return Some(value.clone());
            }

            map.values()
                .find_map(|value| find_string_field(value, field))
        }
        Value::Array(values) => values
            .iter()
            .find_map(|value| find_string_field(value, field)),
        _ => None,
    }
}

fn error_to_string(error: &Value) -> String {
    error
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| error.to_string())
}

fn next_rpc_id(jsonrpc_id: &Arc<AtomicU64>) -> u64 {
    jsonrpc_id.fetch_add(1, Ordering::SeqCst)
}

fn is_quicknode_endpoint(endpoint: &str) -> bool {
    let lower = endpoint.to_ascii_lowercase();

    let valid_scheme = lower.starts_with("https://")
        || lower.starts_with("http://")
        || lower.starts_with("wss://")
        || lower.starts_with("ws://");

    valid_scheme && (lower.contains("quiknode.pro") || lower.contains("quicknode"))
}

fn build_ws_url(endpoint: &str) -> String {
    let base = endpoint.trim_end_matches('/');
    let base = if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{rest}")
    } else if let Some(rest) = base.strip_prefix("http://") {
        format!("ws://{rest}")
    } else {
        base.to_string()
    };

    if base.contains(QUICKNODE_WS_PATH) {
        base
    } else {
        format!("{base}{QUICKNODE_WS_PATH}")
    }
}
