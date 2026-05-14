use std::collections::{HashMap, HashSet};
use std::hash::BuildHasherDefault;

use alloy::primitives::Address;
use rustc_hash::FxHasher;
use tokio::sync::{
    broadcast,
    mpsc::{Receiver, Sender, channel, error::TrySendError},
    oneshot,
};
use tokio::task::JoinHandle;
use tokio::time::{Duration, interval, timeout};
use tokio_util::sync::CancellationToken;

use crate::metrics;
use crate::stream::{
    AccountEvent, AccountFill, AccountFunding, AccountNonFundingLedgerUpdate, EventStream,
};
use crate::{BaseUrl, Error};

type FxMap<K, V> = HashMap<K, V, BuildHasherDefault<FxHasher>>;

pub const QN_BUILD_ENDPOINTS_PER_ACCOUNT: usize = 10;
pub const QN_BUILD_RPS_PER_ACCOUNT: usize = 50;
pub const QN_BUILD_MONTHLY_CREDITS: usize = 80_000_000;
pub const QN_MAX_USER_VALUES_PER_FILTER: usize = 100;
pub const QN_MAX_NAMED_FILTERS_PER_STREAM_TYPE: usize = 10;
pub const QN_MAX_TOTAL_FILTER_VALUES_PER_FILTER: usize = 500;
pub const USERS_PER_SUBSCRIBE_USER_EVENTS_CALL: usize = QN_MAX_USER_VALUES_PER_FILTER;
pub const SUBSCRIBE_USER_EVENTS_CALLS_PER_WS: usize = QN_MAX_NAMED_FILTERS_PER_STREAM_TYPE;
pub const USERS_PER_WS_FOR_FILLS_AND_FUNDINGS: usize =
    USERS_PER_SUBSCRIBE_USER_EVENTS_CALL * SUBSCRIBE_USER_EVENTS_CALLS_PER_WS;
pub const USERS_PER_BUILD_ACCOUNT_FOR_FILLS_AND_FUNDINGS: usize =
    QN_BUILD_ENDPOINTS_PER_ACCOUNT * USERS_PER_WS_FOR_FILLS_AND_FUNDINGS;

const USER_EVENT_CHANNEL_CAPACITY: usize = 256;
const USER_EVENT_RELAY_EVENT_CAPACITY: usize = USER_EVENT_CHANNEL_CAPACITY * 4;
const USER_EVENT_CMD_CHANNEL_CAPACITY: usize = 2048;
const USER_EVENT_CLEANUP_SECS: u64 = 60;
const USER_EVENT_CMD_BATCH_MAX: usize = 512;
const USER_EVENT_SUBSCRIBE_TIMEOUT_SECS: u64 = 5;
const USER_EVENT_UNSUBSCRIBE_TIMEOUT_SECS: u64 = 5;

type UserEventSubReply = Result<Receiver<AccountEvent>, Error>;

struct UserEventSubscribePayload {
    user: Address,
    reply: oneshot::Sender<UserEventSubReply>,
}

enum UserEventCmd {
    Subscribe(UserEventSubscribePayload),
    Unsubscribe(Address),
}

#[derive(Clone)]
pub struct UserEventRelayHandle {
    tx: Sender<UserEventCmd>,
}

impl UserEventRelayHandle {
    pub(crate) async fn subscribe(&self, user: Address) -> UserEventSubReply {
        let (reply, rx) = oneshot::channel();
        timeout(
            Duration::from_secs(USER_EVENT_SUBSCRIBE_TIMEOUT_SECS),
            self.tx
                .send(UserEventCmd::Subscribe(UserEventSubscribePayload {
                    user,
                    reply,
                })),
        )
        .await
        .map_err(|_| Error::Custom("UserEventRelay subscribe timed out".to_string()))?
        .map_err(|err| Error::Custom(format!("UserEventRelay channel closed: {err}")))?;

        timeout(Duration::from_secs(USER_EVENT_SUBSCRIBE_TIMEOUT_SECS), rx)
            .await
            .map_err(|_| Error::Custom("UserEventRelay subscribe reply timed out".to_string()))?
            .map_err(|_| Error::Custom("UserEventRelay subscribe reply dropped".to_string()))?
    }

    pub(crate) fn unsubscribe(&self, user: Address) {
        match self.tx.try_send(UserEventCmd::Unsubscribe(user)) {
            Ok(()) => {}
            Err(TrySendError::Full(cmd)) => {
                let tx = self.tx.clone();
                tokio::spawn(async move {
                    match timeout(
                        Duration::from_secs(USER_EVENT_UNSUBSCRIBE_TIMEOUT_SECS),
                        tx.send(cmd),
                    )
                    .await
                    {
                        Ok(Ok(())) => {}
                        Ok(Err(err)) => {
                            log::warn!(
                                "UserEventRelay channel closed before delayed unsubscribe: {err}"
                            );
                        }
                        Err(_) => {
                            log::warn!("timed out queuing delayed UserEventRelay unsubscribe");
                        }
                    }
                });
                log::warn!("UserEventRelay command queue full while unsubscribing user");
            }
            Err(TrySendError::Closed(_)) => {
                log::warn!("failed to queue UserEventRelay unsubscribe: channel closed");
            }
        }
    }
}

struct UserFeed {
    user: Address,
    tx: broadcast::Sender<AccountEvent>,
    stream_index: usize,
    chunk_index: usize,
}

struct RelayEndpoint {
    endpoint: String,
    stream: Option<EventStream>,
    chunks: Vec<UserChunk>,
}

#[derive(Default)]
struct UserChunk {
    users: Vec<Address>,
    subscription_id: Option<u32>,
    task: Option<JoinHandle<()>>,
}

struct RelayEvent {
    stream_index: usize,
    chunk_index: usize,
    event: AccountEvent,
}

struct PendingSubscribe {
    user_key: String,
    user: Address,
    stream_index: usize,
    chunk_index: usize,
    reply: oneshot::Sender<UserEventSubReply>,
}

pub struct UserEventRelay {
    url: BaseUrl,
    cmd_rx: Receiver<UserEventCmd>,
    event_tx: Sender<RelayEvent>,
    event_rx: Receiver<RelayEvent>,
    endpoints: Vec<RelayEndpoint>,
    users: FxMap<String, UserFeed>,
}

impl UserEventRelay {
    pub async fn from_env(url: BaseUrl) -> Result<Option<(Self, UserEventRelayHandle)>, Error> {
        let endpoints = quicknode_endpoints_from_env();
        if endpoints.is_empty() {
            return Ok(None);
        }

        Self::new(url, endpoints).await.map(Some)
    }

    pub async fn new(
        url: BaseUrl,
        endpoints: Vec<String>,
    ) -> Result<(Self, UserEventRelayHandle), Error> {
        if endpoints.is_empty() {
            return Err(Error::Custom(
                "UserEventRelay requires at least one QuickNode endpoint".to_string(),
            ));
        }

        if endpoints.len() > QN_BUILD_ENDPOINTS_PER_ACCOUNT {
            return Err(Error::Custom(format!(
                "UserEventRelay configured with {} endpoints; max supported is {}",
                endpoints.len(),
                QN_BUILD_ENDPOINTS_PER_ACCOUNT
            )));
        }

        let endpoints = endpoints
            .into_iter()
            .map(|endpoint| RelayEndpoint {
                endpoint,
                stream: None,
                chunks: Vec::new(),
            })
            .collect::<Vec<_>>();
        let (cmd_tx, cmd_rx) = channel(USER_EVENT_CMD_CHANNEL_CAPACITY);
        let (event_tx, event_rx) = channel(USER_EVENT_RELAY_EVENT_CAPACITY);

        Ok((
            Self {
                url,
                cmd_rx,
                event_tx,
                event_rx,
                endpoints,
                users: HashMap::default(),
            },
            UserEventRelayHandle { tx: cmd_tx },
        ))
    }

    pub async fn start(&mut self, shutdown: CancellationToken) {
        let mut cleanup = interval(Duration::from_secs(USER_EVENT_CLEANUP_SECS));
        cleanup.tick().await;

        loop {
            tokio::select! {
                _ = shutdown.cancelled() => {
                    break;
                }
                _ = cleanup.tick() => {
                    self.cleanup_idle_users().await;
                }
                maybe_cmd = self.cmd_rx.recv() => {
                    let Some(cmd) = maybe_cmd else {
                        break;
                    };
                    self.handle_cmd_batch(cmd).await;
                }
                maybe_event = self.event_rx.recv() => {
                    let Some(event) = maybe_event else {
                        break;
                    };
                    self.route_event(event);
                }
            }
        }
    }

    async fn handle_cmd_batch(&mut self, first: UserEventCmd) {
        let mut cmds = vec![first];
        while cmds.len() < USER_EVENT_CMD_BATCH_MAX {
            match self.cmd_rx.try_recv() {
                Ok(cmd) => cmds.push(cmd),
                Err(_) => break,
            }
        }

        let mut touched_chunks = HashSet::new();
        let mut pending_subscribes = Vec::new();

        for cmd in cmds {
            match cmd {
                UserEventCmd::Subscribe(payload) => {
                    let user_key = user_key(&payload.user);

                    if let Some(feed) = self.users.get(&user_key) {
                        let rx = bounded_from_broadcast(feed.tx.subscribe());
                        let _ = payload.reply.send(Ok(rx));
                        continue;
                    }

                    let Ok((stream_index, chunk_index)) = self.reserve_slot(payload.user) else {
                        let _ = payload.reply.send(Err(Error::Custom(format!(
                            "UserEventRelay capacity exceeded: max {} users across {} QuickNode endpoints",
                            USERS_PER_BUILD_ACCOUNT_FOR_FILLS_AND_FUNDINGS, QN_BUILD_ENDPOINTS_PER_ACCOUNT
                        ))));
                        continue;
                    };

                    let (tx, rx) = broadcast::channel(USER_EVENT_CHANNEL_CAPACITY);
                    self.users.insert(
                        user_key.clone(),
                        UserFeed {
                            user: payload.user,
                            tx,
                            stream_index,
                            chunk_index,
                        },
                    );
                    drop(rx);

                    touched_chunks.insert((stream_index, chunk_index));
                    pending_subscribes.push(PendingSubscribe {
                        user_key,
                        user: payload.user,
                        stream_index,
                        chunk_index,
                        reply: payload.reply,
                    });
                }

                UserEventCmd::Unsubscribe(user) => {
                    let key = user_key(&user);
                    let Some(feed) = self.users.remove(&key) else {
                        continue;
                    };

                    self.remove_user_from_chunk(feed.stream_index, feed.chunk_index, user);
                    touched_chunks.insert((feed.stream_index, feed.chunk_index));
                }
            }
        }

        let mut results = HashMap::new();
        for (stream_index, chunk_index) in touched_chunks {
            let result = self
                .resubscribe_chunk(stream_index, chunk_index)
                .await
                .map_err(|err| err.to_string());
            if let Err(err) = &result {
                log::warn!(
                    "failed to resubscribe QuickNode user-event chunk {stream_index}/{chunk_index}: {err}"
                );
            }
            results.insert((stream_index, chunk_index), result);
        }

        let mut failed_reply_chunks = HashSet::new();

        for pending in pending_subscribes {
            match results.get(&(pending.stream_index, pending.chunk_index)) {
                Some(Ok(())) => {
                    if let Some(feed) = self.users.get(&pending.user_key) {
                        if pending
                            .reply
                            .send(Ok(bounded_from_broadcast(feed.tx.subscribe())))
                            .is_err()
                        {
                            self.users.remove(&pending.user_key);
                            self.remove_user_from_chunk(
                                pending.stream_index,
                                pending.chunk_index,
                                pending.user,
                            );
                            failed_reply_chunks.insert((pending.stream_index, pending.chunk_index));
                        }
                    } else {
                        let _ = pending.reply.send(Err(Error::Custom(
                            "UserEventRelay feed removed before subscribe completed".to_string(),
                        )));
                    }
                }
                Some(Err(err)) => {
                    self.users.remove(&pending.user_key);
                    self.remove_user_from_chunk(
                        pending.stream_index,
                        pending.chunk_index,
                        pending.user,
                    );
                    let _ = pending.reply.send(Err(Error::Custom(err.clone())));
                }
                None => {
                    let _ = pending.reply.send(Err(Error::Custom(
                        "UserEventRelay chunk was not resubscribed".to_string(),
                    )));
                }
            }
        }

        for (stream_index, chunk_index) in failed_reply_chunks {
            if let Err(err) = self.resubscribe_chunk(stream_index, chunk_index).await {
                log::warn!(
                    "failed to resubscribe QuickNode user-event chunk after dropped subscribe reply: {err}"
                );
            }
        }
    }

    async fn cleanup_idle_users(&mut self) {
        let users = self
            .users
            .iter()
            .filter(|(_, feed)| feed.tx.receiver_count() == 0)
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();

        let mut touched_chunks = HashSet::new();

        for key in users {
            if let Some(feed) = self.users.remove(&key) {
                self.remove_user_from_chunk(feed.stream_index, feed.chunk_index, feed.user);
                touched_chunks.insert((feed.stream_index, feed.chunk_index));
            }
        }

        for (stream_index, endpoint) in self.endpoints.iter().enumerate() {
            for (chunk_index, chunk) in endpoint.chunks.iter().enumerate() {
                if !chunk.users.is_empty() && chunk.subscription_id.is_none() {
                    touched_chunks.insert((stream_index, chunk_index));
                }
            }
        }

        for (stream_index, chunk_index) in touched_chunks {
            if let Err(err) = self.resubscribe_chunk(stream_index, chunk_index).await {
                log::warn!("failed to resubscribe QuickNode user-event chunk after cleanup: {err}");
            }
        }
    }

    fn reserve_slot(&mut self, user: Address) -> Result<(usize, usize), Error> {
        for (stream_index, endpoint) in self.endpoints.iter_mut().enumerate() {
            for (chunk_index, chunk) in endpoint.chunks.iter_mut().enumerate() {
                if chunk.users.len() < USERS_PER_SUBSCRIBE_USER_EVENTS_CALL {
                    chunk.users.push(user);
                    return Ok((stream_index, chunk_index));
                }
            }

            if endpoint.chunks.len() < SUBSCRIBE_USER_EVENTS_CALLS_PER_WS {
                endpoint.chunks.push(UserChunk {
                    users: vec![user],
                    subscription_id: None,
                    task: None,
                });
                return Ok((stream_index, endpoint.chunks.len() - 1));
            }
        }

        Err(Error::Custom(
            "UserEventRelay capacity exceeded".to_string(),
        ))
    }

    fn remove_user_from_chunk(&mut self, stream_index: usize, chunk_index: usize, user: Address) {
        if let Some(chunk) = self
            .endpoints
            .get_mut(stream_index)
            .and_then(|endpoint| endpoint.chunks.get_mut(chunk_index))
        {
            chunk.users.retain(|existing| *existing != user);
        }
    }

    async fn resubscribe_chunk(
        &mut self,
        stream_index: usize,
        chunk_index: usize,
    ) -> Result<(), Error> {
        if self.endpoints[stream_index].stream.is_none() {
            let endpoint = self.endpoints[stream_index].endpoint.clone();
            let stream = EventStream::new(endpoint, true, self.url).await?;
            self.endpoints[stream_index].stream = Some(stream);
        }

        let users = self.endpoints[stream_index].chunks[chunk_index]
            .users
            .clone();

        if users.is_empty() {
            let (old_subscription_id, old_task) = {
                let chunk = &mut self.endpoints[stream_index].chunks[chunk_index];
                (chunk.subscription_id.take(), chunk.task.take())
            };

            if let Some(task) = old_task {
                task.abort();
            }

            if let Some(subscription_id) = old_subscription_id
                && let Some(stream) = self.endpoints[stream_index].stream.as_mut()
            {
                stream.remove_subscription(subscription_id).await?;
            }

            return Ok(());
        }

        let active_chunks = self.endpoints[stream_index]
            .chunks
            .iter()
            .filter(|chunk| chunk.subscription_id.is_some())
            .count();
        let should_remove_old_first = active_chunks >= SUBSCRIBE_USER_EVENTS_CALLS_PER_WS
            && self.endpoints[stream_index].chunks[chunk_index]
                .subscription_id
                .is_some();

        if should_remove_old_first {
            let (old_subscription_id, old_task) = {
                let chunk = &mut self.endpoints[stream_index].chunks[chunk_index];
                (chunk.subscription_id.take(), chunk.task.take())
            };

            if let Some(task) = old_task {
                task.abort();
            }

            if let Some(subscription_id) = old_subscription_id
                && let Some(stream) = self.endpoints[stream_index].stream.as_mut()
                && let Err(err) = stream.remove_subscription(subscription_id).await
            {
                log::warn!(
                    "failed to remove old QuickNode user-event subscription {subscription_id}: {err}"
                );
            }
        }

        let (tx, mut rx) = channel(USER_EVENT_RELAY_EVENT_CAPACITY);
        let subscription_id = self.endpoints[stream_index]
            .stream
            .as_mut()
            .ok_or_else(|| Error::Custom("QuickNode EventStream not initialized".to_string()))?
            .subscribe_user_events(users, tx)
            .await?;
        let event_tx = self.event_tx.clone();
        let task = tokio::spawn(async move {
            let mut queue_full = false;
            let mut dropped = 0_u64;
            while let Some(event) = rx.recv().await {
                match event_tx.try_send(RelayEvent {
                    stream_index,
                    chunk_index,
                    event,
                }) {
                    Ok(()) => {
                        if queue_full {
                            log::info!(
                                "UserEventRelay event queue recovered after dropping {dropped} events"
                            );
                            queue_full = false;
                            dropped = 0;
                        }
                    }
                    Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                        metrics::inc_user_event_relay_event_dropped();
                        dropped = dropped.saturating_add(1);
                        if !queue_full {
                            log::warn!("UserEventRelay event queue full; dropping account events");
                            queue_full = true;
                        }
                    }
                    Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => break,
                }
            }
        });

        let (old_subscription_id, old_task) = {
            let chunk = &mut self.endpoints[stream_index].chunks[chunk_index];
            let old_subscription_id = chunk.subscription_id.replace(subscription_id);
            let old_task = chunk.task.replace(task);
            (old_subscription_id, old_task)
        };

        if let Some(task) = old_task {
            task.abort();
        }

        if let Some(subscription_id) = old_subscription_id
            && let Some(stream) = self.endpoints[stream_index].stream.as_mut()
            && let Err(err) = stream.remove_subscription(subscription_id).await
        {
            log::warn!(
                "failed to remove old QuickNode user-event subscription {subscription_id}: {err}"
            );
        }

        Ok(())
    }

    fn route_event(&self, relay_event: RelayEvent) {
        match relay_event.event {
            AccountEvent::Fill(fills) => {
                for (user, fills) in group_fills_by_user(fills) {
                    self.send_to_user(&user, AccountEvent::Fill(fills));
                }
            }
            AccountEvent::Funding(fundings) => {
                for (user, fundings) in group_fundings_by_user(fundings) {
                    self.send_to_user(&user, AccountEvent::Funding(fundings));
                }
            }
            AccountEvent::NonFundingLedgerUpdates(updates) => {
                for (user, updates) in group_non_funding_ledger_updates_by_user(updates) {
                    self.send_to_user(&user, AccountEvent::NonFundingLedgerUpdates(updates));
                }
            }
            event @ (AccountEvent::Raw { .. } | AccountEvent::Error(_) | AccountEvent::NoData) => {
                self.send_to_chunk(relay_event.stream_index, relay_event.chunk_index, event);
            }
        }
    }

    fn send_to_user(&self, user: &Address, event: AccountEvent) {
        if let Some(feed) = self.users.get(&user_key(user)) {
            let _ = feed.tx.send(event);
        }
    }

    fn send_to_chunk(&self, stream_index: usize, chunk_index: usize, event: AccountEvent) {
        let Some(chunk) = self
            .endpoints
            .get(stream_index)
            .and_then(|endpoint| endpoint.chunks.get(chunk_index))
        else {
            return;
        };

        for user in &chunk.users {
            self.send_to_user(user, event.clone());
        }
    }
}

fn quicknode_endpoints_from_env() -> Vec<String> {
    let mut raw_values = Vec::with_capacity(QN_BUILD_ENDPOINTS_PER_ACCOUNT + 3);

    if let Ok(raw) = std::env::var("QUICKNODE_HYPERCORE_ENDPOINTS") {
        raw_values.push(raw);
    }

    for index in 1..=QN_BUILD_ENDPOINTS_PER_ACCOUNT {
        if let Ok(raw) = std::env::var(format!("QUICKNODE_HYPERCORE_ENDPOINT{index}")) {
            raw_values.push(raw);
        }
    }

    for key in ["QUICKNODE_HYPERCORE_ENDPOINT", "QUICKNODE_ENDPOINT"] {
        if let Ok(raw) = std::env::var(key) {
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

fn bounded_from_broadcast(mut rx: broadcast::Receiver<AccountEvent>) -> Receiver<AccountEvent> {
    let (tx, out_rx) = channel(USER_EVENT_CHANNEL_CAPACITY);

    tokio::spawn(async move {
        let mut queue_full = false;
        let mut dropped = 0_u64;
        loop {
            match rx.recv().await {
                Ok(event) => match tx.try_send(event) {
                    Ok(()) => {
                        if queue_full {
                            log::info!(
                                "UserEventRelay subscriber queue recovered after dropping {dropped} events"
                            );
                            queue_full = false;
                            dropped = 0;
                        }
                    }
                    Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                        metrics::inc_user_event_relay_subscriber_dropped();
                        dropped = dropped.saturating_add(1);
                        if !queue_full {
                            log::warn!(
                                "UserEventRelay subscriber queue full; dropping account events"
                            );
                            queue_full = true;
                        }
                    }
                    Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => break,
                },
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    metrics::add_user_event_relay_lagged(n);
                    match tx.try_send(AccountEvent::Error(format!(
                        "UserEventRelay receiver lagged by {n} events"
                    ))) {
                        Ok(()) => {}
                        Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                            metrics::inc_user_event_relay_subscriber_dropped();
                            if !queue_full {
                                log::warn!(
                                    "UserEventRelay subscriber queue full; dropping lag warning"
                                );
                                queue_full = true;
                            }
                        }
                        Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => break,
                    }
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    out_rx
}

fn group_fills_by_user(fills: Vec<AccountFill>) -> Vec<(Address, Vec<AccountFill>)> {
    let mut grouped = FxMap::<String, (Address, Vec<AccountFill>)>::default();

    for fill in fills {
        grouped
            .entry(user_key(&fill.user))
            .or_insert_with(|| (fill.user, Vec::new()))
            .1
            .push(fill);
    }

    grouped.into_values().collect()
}

fn group_fundings_by_user(fundings: Vec<AccountFunding>) -> Vec<(Address, Vec<AccountFunding>)> {
    let mut grouped = FxMap::<String, (Address, Vec<AccountFunding>)>::default();

    for funding in fundings {
        grouped
            .entry(user_key(&funding.user))
            .or_insert_with(|| (funding.user, Vec::new()))
            .1
            .push(funding);
    }

    grouped.into_values().collect()
}

fn group_non_funding_ledger_updates_by_user(
    updates: Vec<AccountNonFundingLedgerUpdate>,
) -> Vec<(Address, Vec<AccountNonFundingLedgerUpdate>)> {
    let mut grouped = FxMap::<String, (Address, Vec<AccountNonFundingLedgerUpdate>)>::default();

    for update in updates {
        grouped
            .entry(user_key(&update.user))
            .or_insert_with(|| (update.user, Vec::new()))
            .1
            .push(update);
    }

    grouped.into_values().collect()
}

fn user_key(user: &Address) -> String {
    format!("{user:#x}").to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn address(index: usize) -> Address {
        let mut bytes = [0_u8; 20];
        bytes[12..].copy_from_slice(&(index as u64).to_be_bytes());
        Address::from(bytes)
    }

    fn endpoints(count: usize) -> Vec<String> {
        (0..count)
            .map(|index| format!("https://relay-test-{index}.quicknode.example"))
            .collect()
    }

    async fn test_relay(endpoint_count: usize) -> UserEventRelay {
        UserEventRelay::new(BaseUrl::Mainnet, endpoints(endpoint_count))
            .await
            .expect("test relay should initialize")
            .0
    }

    #[tokio::test]
    async fn reserve_slot_fills_build_account_capacity_and_then_rejects() {
        let mut relay = test_relay(QN_BUILD_ENDPOINTS_PER_ACCOUNT).await;

        for index in 0..USERS_PER_BUILD_ACCOUNT_FOR_FILLS_AND_FUNDINGS {
            let (stream_index, chunk_index) = relay
                .reserve_slot(address(index))
                .expect("slot should be available");
            let expected_stream = index / USERS_PER_WS_FOR_FILLS_AND_FUNDINGS;
            let within_stream = index % USERS_PER_WS_FOR_FILLS_AND_FUNDINGS;
            let expected_chunk = within_stream / USERS_PER_SUBSCRIBE_USER_EVENTS_CALL;

            assert_eq!(stream_index, expected_stream);
            assert_eq!(chunk_index, expected_chunk);
        }

        assert!(
            relay
                .reserve_slot(address(USERS_PER_BUILD_ACCOUNT_FOR_FILLS_AND_FUNDINGS))
                .is_err()
        );

        for endpoint in &relay.endpoints {
            assert_eq!(endpoint.chunks.len(), SUBSCRIBE_USER_EVENTS_CALLS_PER_WS);
            assert!(
                endpoint
                    .chunks
                    .iter()
                    .all(|chunk| chunk.users.len() == USERS_PER_SUBSCRIBE_USER_EVENTS_CALL)
            );
        }
    }

    #[tokio::test]
    async fn reserve_slot_reuses_freed_space_before_creating_new_chunks() {
        let mut relay = test_relay(1).await;

        for index in 0..(USERS_PER_SUBSCRIBE_USER_EVENTS_CALL * 2) {
            relay
                .reserve_slot(address(index))
                .expect("initial slots should be available");
        }

        let removed = address(42);
        relay.remove_user_from_chunk(0, 0, removed);
        assert_eq!(
            relay.endpoints[0].chunks[0].users.len(),
            USERS_PER_SUBSCRIBE_USER_EVENTS_CALL - 1
        );

        let new_user = address(10_000);
        let slot = relay
            .reserve_slot(new_user)
            .expect("freed slot should be reused");

        assert_eq!(slot, (0, 0));
        assert_eq!(
            relay.endpoints[0].chunks[0].users.len(),
            USERS_PER_SUBSCRIBE_USER_EVENTS_CALL
        );
        assert_eq!(relay.endpoints[0].chunks.len(), 2);
        assert!(relay.endpoints[0].chunks[0].users.contains(&new_user));
    }

    #[tokio::test]
    async fn new_rejects_more_than_build_endpoint_limit() {
        let result = UserEventRelay::new(
            BaseUrl::Mainnet,
            endpoints(QN_BUILD_ENDPOINTS_PER_ACCOUNT + 1),
        )
        .await;

        assert!(result.is_err());
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

    #[tokio::test]
    async fn bounded_broadcast_bridge_drops_overflow_and_accepts_new_events() {
        let (tx, _) = broadcast::channel(USER_EVENT_CHANNEL_CAPACITY * 4);
        let mut rx = bounded_from_broadcast(tx.subscribe());

        for _ in 0..(USER_EVENT_CHANNEL_CAPACITY * 2) {
            tx.send(AccountEvent::NoData)
                .expect("broadcast receiver should be open");
        }

        tokio::time::timeout(Duration::from_secs(1), async {
            while rx.len() < USER_EVENT_CHANNEL_CAPACITY {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("bounded bridge should fill subscriber queue");

        while rx.try_recv().is_ok() {}

        tx.send(AccountEvent::Error("marker".to_string()))
            .expect("broadcast receiver should still be open");

        let marker_seen = tokio::time::timeout(Duration::from_secs(1), async {
            while let Some(event) = rx.recv().await {
                if matches!(event, AccountEvent::Error(message) if message == "marker") {
                    return true;
                }
            }
            false
        })
        .await
        .expect("marker event should arrive");

        assert!(marker_seen);
    }
}
