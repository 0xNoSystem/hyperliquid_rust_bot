use std::collections::HashMap;
use std::hash::BuildHasherDefault;

use alloy::primitives::Address;
use rustc_hash::FxHasher;
use tokio::sync::{
    broadcast,
    mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
    oneshot,
};
use tokio::task::JoinHandle;
use tokio::time::{Duration, interval};

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
const USER_EVENT_CLEANUP_SECS: u64 = 60;

type UserEventSubReply = Result<UnboundedReceiver<AccountEvent>, Error>;

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
    tx: UnboundedSender<UserEventCmd>,
}

impl UserEventRelayHandle {
    pub(crate) async fn subscribe(&self, user: Address) -> UserEventSubReply {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(UserEventCmd::Subscribe(UserEventSubscribePayload {
                user,
                reply,
            }))
            .map_err(|err| Error::Custom(format!("UserEventRelay channel closed: {err}")))?;

        rx.await
            .map_err(|_| Error::Custom("UserEventRelay subscribe reply dropped".to_string()))?
    }

    pub(crate) fn unsubscribe(&self, user: Address) {
        let _ = self.tx.send(UserEventCmd::Unsubscribe(user));
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

pub struct UserEventRelay {
    url: BaseUrl,
    cmd_rx: UnboundedReceiver<UserEventCmd>,
    event_tx: UnboundedSender<RelayEvent>,
    event_rx: UnboundedReceiver<RelayEvent>,
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
        let (cmd_tx, cmd_rx) = unbounded_channel();
        let (event_tx, event_rx) = unbounded_channel();

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

    pub async fn start(&mut self) {
        let mut cleanup = interval(Duration::from_secs(USER_EVENT_CLEANUP_SECS));
        cleanup.tick().await;

        loop {
            tokio::select! {
                _ = cleanup.tick() => {
                    self.cleanup_idle_users().await;
                }
                maybe_cmd = self.cmd_rx.recv() => {
                    let Some(cmd) = maybe_cmd else {
                        break;
                    };
                    match cmd {
                        UserEventCmd::Subscribe(payload) => self.subscribe(payload).await,
                        UserEventCmd::Unsubscribe(user) => self.unsubscribe(user).await,
                    }
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

    async fn subscribe(&mut self, payload: UserEventSubscribePayload) {
        let user_key = user_key(&payload.user);

        if let Some(feed) = self.users.get(&user_key) {
            let rx = unbounded_from_broadcast(feed.tx.subscribe());
            let _ = payload.reply.send(Ok(rx));
            return;
        }

        let Ok((stream_index, chunk_index)) = self.reserve_slot(payload.user) else {
            let _ = payload.reply.send(Err(Error::Custom(format!(
                "UserEventRelay capacity exceeded: max {} users across {} QuickNode endpoints",
                USERS_PER_BUILD_ACCOUNT_FOR_FILLS_AND_FUNDINGS, QN_BUILD_ENDPOINTS_PER_ACCOUNT
            ))));
            return;
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

        match self.resubscribe_chunk(stream_index, chunk_index).await {
            Ok(()) => {
                if let Some(feed) = self.users.get(&user_key) {
                    let _ = payload
                        .reply
                        .send(Ok(unbounded_from_broadcast(feed.tx.subscribe())));
                } else {
                    let _ = payload.reply.send(Err(Error::Custom(
                        "UserEventRelay feed missing after subscribe".to_string(),
                    )));
                }
            }
            Err(err) => {
                self.users.remove(&user_key);
                self.remove_user_from_chunk(stream_index, chunk_index, payload.user);
                let _ = payload.reply.send(Err(err));
            }
        }

        drop(rx);
    }

    async fn unsubscribe(&mut self, user: Address) {
        let key = user_key(&user);
        let Some(feed) = self.users.remove(&key) else {
            return;
        };

        self.remove_user_from_chunk(feed.stream_index, feed.chunk_index, user);
        if let Err(err) = self
            .resubscribe_chunk(feed.stream_index, feed.chunk_index)
            .await
        {
            log::warn!("failed to resubscribe QuickNode user-event chunk after unsubscribe: {err}");
        }
    }

    async fn cleanup_idle_users(&mut self) {
        let users = self
            .users
            .iter()
            .filter(|(_, feed)| feed.tx.receiver_count() == 0)
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();

        for key in users {
            if let Some(feed) = self.users.remove(&key) {
                self.remove_user_from_chunk(feed.stream_index, feed.chunk_index, feed.user);
                if let Err(err) = self
                    .resubscribe_chunk(feed.stream_index, feed.chunk_index)
                    .await
                {
                    log::warn!(
                        "failed to resubscribe QuickNode user-event chunk after cleanup: {err}"
                    );
                }
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

        let (old_subscription_id, old_task, users) = {
            let chunk = &mut self.endpoints[stream_index].chunks[chunk_index];
            (
                chunk.subscription_id.take(),
                chunk.task.take(),
                chunk.users.clone(),
            )
        };

        if let Some(task) = old_task {
            task.abort();
        }

        if let Some(subscription_id) = old_subscription_id
            && let Some(stream) = self.endpoints[stream_index].stream.as_mut()
        {
            stream.remove_subscription(subscription_id).await?;
        }

        if users.is_empty() {
            return Ok(());
        }

        let (tx, mut rx) = unbounded_channel();
        let subscription_id = self.endpoints[stream_index]
            .stream
            .as_mut()
            .ok_or_else(|| Error::Custom("QuickNode EventStream not initialized".to_string()))?
            .subscribe_user_events(users, tx)
            .await?;
        let event_tx = self.event_tx.clone();
        let task = tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                if event_tx
                    .send(RelayEvent {
                        stream_index,
                        chunk_index,
                        event,
                    })
                    .is_err()
                {
                    break;
                }
            }
        });

        let chunk = &mut self.endpoints[stream_index].chunks[chunk_index];
        chunk.subscription_id = Some(subscription_id);
        chunk.task = Some(task);

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
    [
        "QUICKNODE_HYPERCORE_ENDPOINTS",
        "QUICKNODE_HYPERCORE_ENDPOINT",
        "QUICKNODE_ENDPOINT",
    ]
    .into_iter()
    .find_map(|key| std::env::var(key).ok())
    .map(|raw| {
        raw.split(',')
            .map(str::trim)
            .filter(|endpoint| !endpoint.is_empty())
            .map(ToOwned::to_owned)
            .collect()
    })
    .unwrap_or_default()
}

fn unbounded_from_broadcast(
    mut rx: broadcast::Receiver<AccountEvent>,
) -> UnboundedReceiver<AccountEvent> {
    let (tx, out_rx) = unbounded_channel();

    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    if tx.send(event).is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    if tx
                        .send(AccountEvent::Error(format!(
                            "UserEventRelay receiver lagged by {n} events"
                        )))
                        .is_err()
                    {
                        break;
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
}
