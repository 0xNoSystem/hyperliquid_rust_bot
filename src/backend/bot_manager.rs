use std::collections::HashMap;
use std::time::Duration;

use alloy::signers::local::PrivateKeySigner;
use futures_util::future::join_all;
use log::{info, warn};
use sqlx::PgPool;
use tokio::sync::mpsc::{Sender, error::TrySendError};
use tokio::task::JoinHandle;
use tokio::time::timeout;

use crate::broadcast::{BroadcastCmd, CacheCmdIn, UserEventRelayHandle};
use crate::{BaseUrl, Bot, BotEvent, Wallet};

const MANAGER_EVENT_SEND_TIMEOUT_SECS: u64 = 5;
const MANAGER_DB_QUERY_TIMEOUT_SECS: u64 = 10;
const MANAGER_BOT_JOIN_TIMEOUT_SECS: u64 = 60;

struct BotHandle {
    cmd_tx: Sender<BotEvent>,
    task: Option<JoinHandle<()>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ManagerEventSend {
    Queued,
    Closed,
    TimedOut,
}

#[derive(Clone)]
pub(crate) struct BotBuildContext {
    broadcast_tx: Sender<BroadcastCmd>,
    cache_tx: Sender<CacheCmdIn>,
    user_event_relay: Option<UserEventRelayHandle>,
}

pub struct BotManager {
    bots: HashMap<String, BotHandle>,
    broadcast_tx: Sender<BroadcastCmd>,
    cache_tx: Sender<CacheCmdIn>,
    user_event_relay: Option<UserEventRelayHandle>,
}

impl BotManager {
    pub fn new(
        broadcast_tx: Sender<BroadcastCmd>,
        cache_tx: Sender<CacheCmdIn>,
        user_event_relay: Option<UserEventRelayHandle>,
    ) -> Self {
        Self {
            bots: HashMap::new(),
            broadcast_tx,
            cache_tx,
            user_event_relay,
        }
    }

    /// Get the command sender for an existing bot.
    pub fn get_bot(&self, pubkey: &str) -> Option<Sender<BotEvent>> {
        self.bots.get(pubkey).map(|h| h.cmd_tx.clone())
    }

    pub(crate) fn build_context(&self) -> BotBuildContext {
        BotBuildContext {
            broadcast_tx: self.broadcast_tx.clone(),
            cache_tx: self.cache_tx.clone(),
            user_event_relay: self.user_event_relay.clone(),
        }
    }

    pub(crate) fn register_bot_if_absent(
        &mut self,
        pubkey: String,
        cmd_tx: Sender<BotEvent>,
    ) -> Sender<BotEvent> {
        if let Some(handle) = self.bots.get(&pubkey)
            && !handle.cmd_tx.is_closed()
        {
            return handle.cmd_tx.clone();
        }

        self.bots.remove(&pubkey);
        self.bots.insert(
            pubkey.clone(),
            BotHandle {
                cmd_tx: cmd_tx.clone(),
                task: None,
            },
        );
        info!("Created bot for user {}", pubkey);
        cmd_tx
    }

    pub(crate) fn attach_bot_task(
        &mut self,
        pubkey: &str,
        cmd_tx: &Sender<BotEvent>,
        task: JoinHandle<()>,
    ) -> bool {
        let Some(handle) = self.bots.get_mut(pubkey) else {
            task.abort();
            return false;
        };

        if !handle.cmd_tx.same_channel(cmd_tx) {
            task.abort();
            return false;
        }

        handle.task = Some(task);
        true
    }

    pub(crate) fn remove_if_sender(&mut self, pubkey: &str, cmd_tx: &Sender<BotEvent>) -> bool {
        let should_remove = self
            .bots
            .get(pubkey)
            .is_some_and(|handle| handle.cmd_tx.same_channel(cmd_tx));

        if should_remove {
            self.bots.remove(pubkey);
            info!("Cleaned up stopped bot for user {}", pubkey);
        }

        should_remove
    }

    /// Hot-reload the wallet for an existing bot (e.g. after agent re-approval).
    /// Returns true if the bot existed and was notified.
    pub async fn reload_wallet(&self, pubkey: &str, signer: PrivateKeySigner) -> bool {
        if let Some(handle) = self.bots.get(pubkey) {
            if handle.cmd_tx.is_closed() {
                return false;
            }
            queue_manager_event(
                pubkey,
                &handle.cmd_tx,
                BotEvent::ReloadWallet(signer),
                "ReloadWallet",
            )
            .await
            .is_queued()
        } else {
            false
        }
    }

    /// Remove a bot for a given user (sends Kill event first).
    pub async fn remove_bot(&mut self, pubkey: &str) {
        let Some(cmd_tx) = self.bots.get(pubkey).map(|handle| handle.cmd_tx.clone()) else {
            return;
        };

        match queue_manager_event(pubkey, &cmd_tx, BotEvent::Kill, "Kill").await {
            ManagerEventSend::Queued | ManagerEventSend::Closed => {
                if let Some(handle) = self.bots.remove(pubkey) {
                    if handle.cmd_tx.same_channel(&cmd_tx) {
                        drop(handle.task);
                        info!("Removed bot for user {}", pubkey);
                    } else {
                        self.bots.insert(pubkey.to_string(), handle);
                    }
                }
            }
            ManagerEventSend::TimedOut => {
                warn!("Keeping bot for user {pubkey} registered because Kill could not be queued");
            }
        }
    }

    /// Shut down all bots gracefully.
    pub async fn shutdown_all(&mut self) {
        Self::shutdown_senders(self.drain_shutdown_senders()).await;
    }

    pub fn drain_shutdown_senders(
        &mut self,
    ) -> Vec<(String, Sender<BotEvent>, Option<JoinHandle<()>>)> {
        self.bots
            .drain()
            .map(|(pubkey, handle)| (pubkey, handle.cmd_tx, handle.task))
            .collect()
    }

    pub async fn shutdown_senders(
        handles: Vec<(String, Sender<BotEvent>, Option<JoinHandle<()>>)>,
    ) {
        let results = join_all(
            handles
                .into_iter()
                .map(|(pubkey, cmd_tx, task)| async move {
                    let result =
                        queue_manager_event(&pubkey, &cmd_tx, BotEvent::Kill, "Kill").await;
                    (pubkey, result, task)
                }),
        )
        .await;

        join_all(
            results
                .into_iter()
                .map(|(pubkey, result, task)| async move {
                    match result {
                        ManagerEventSend::Queued | ManagerEventSend::Closed => {
                            info!("Shut down bot for user {pubkey}");
                            join_bot_task(pubkey, task).await;
                        }
                        ManagerEventSend::TimedOut => {
                            warn!("Timed out shutting down bot for user {pubkey}");
                            abort_bot_task(pubkey, task).await;
                        }
                    }
                }),
        )
        .await;
    }
}

impl ManagerEventSend {
    fn is_queued(self) -> bool {
        matches!(self, Self::Queued)
    }
}

async fn queue_manager_event(
    pubkey: &str,
    tx: &Sender<BotEvent>,
    event: BotEvent,
    label: &'static str,
) -> ManagerEventSend {
    match tx.try_send(event) {
        Ok(()) => {
            info!("Queued {label} for bot user {pubkey}");
            ManagerEventSend::Queued
        }
        Err(TrySendError::Full(event)) => {
            match tokio::time::timeout(
                Duration::from_secs(MANAGER_EVENT_SEND_TIMEOUT_SECS),
                tx.send(event),
            )
            .await
            {
                Ok(Ok(())) => {
                    info!("Queued delayed {label} for bot user {pubkey}");
                    ManagerEventSend::Queued
                }
                Ok(Err(_)) => {
                    warn!("Bot channel closed before delayed {label} for {pubkey}");
                    ManagerEventSend::Closed
                }
                Err(_) => {
                    warn!("Timed out queuing delayed {label} for bot user {pubkey}");
                    ManagerEventSend::TimedOut
                }
            }
        }
        Err(TrySendError::Closed(_)) => ManagerEventSend::Closed,
    }
}

async fn join_bot_task(pubkey: String, task: Option<JoinHandle<()>>) {
    let Some(mut task) = task else {
        return;
    };

    match timeout(
        Duration::from_secs(MANAGER_BOT_JOIN_TIMEOUT_SECS),
        &mut task,
    )
    .await
    {
        Ok(Ok(())) => {}
        Ok(Err(err)) => warn!("bot task for user {pubkey} failed during shutdown: {err}"),
        Err(_) => {
            warn!("timed out waiting for bot task for user {pubkey}; aborting task");
            task.abort();
            let _ = task.await;
        }
    }
}

async fn abort_bot_task(pubkey: String, task: Option<JoinHandle<()>>) {
    let Some(task) = task else {
        return;
    };

    warn!("aborting bot task for user {pubkey}");
    task.abort();
    let _ = task.await;
}

impl BotBuildContext {
    pub(crate) async fn build_bot(
        &self,
        pubkey: &str,
        pool: &PgPool,
        encryption_key: &[u8; 32],
    ) -> Result<(Bot, Sender<BotEvent>), crate::Error> {
        let row = tokio::time::timeout(
            Duration::from_secs(MANAGER_DB_QUERY_TIMEOUT_SECS),
            sqlx::query_scalar::<_, Option<Vec<u8>>>(
                "SELECT api_key_enc FROM users WHERE pubkey = $1",
            )
            .bind(pubkey)
            .fetch_optional(pool),
        )
        .await
        .map_err(|_| crate::Error::Custom("DB query timed out fetching API key".to_string()))?
        .map_err(|e| crate::Error::Custom(format!("DB error: {}", e)))?
        .flatten()
        .ok_or_else(|| crate::Error::Custom("user has no API key set".to_string()))?;

        let decrypted = super::crypto::decrypt(encryption_key, &row)?;
        let private_key_str = String::from_utf8(decrypted)
            .map_err(|e| crate::Error::Custom(format!("invalid UTF-8 key: {}", e)))?;

        let signer: PrivateKeySigner = private_key_str
            .trim()
            .parse()
            .map_err(|e| crate::Error::Custom(format!("invalid private key: {}", e)))?;

        let url = BaseUrl::Mainnet;
        let user_address = crate::helper::address(pubkey)?;
        let wallet = Wallet::new(url, user_address, signer).await?;

        Bot::new(
            wallet,
            self.broadcast_tx.clone(),
            self.cache_tx.clone(),
            self.user_event_relay.clone(),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn queue_manager_event_waits_for_capacity_when_channel_is_full() {
        let (tx, mut rx) = mpsc::channel(1);
        tx.try_send(BotEvent::Kill)
            .expect("initial send should fit");

        let consumer = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            let mut consumed = 0;
            while rx.recv().await.is_some() {
                consumed += 1;
                if consumed == 2 {
                    break;
                }
            }
            consumed
        });

        let result = queue_manager_event("user", &tx, BotEvent::Kill, "Kill").await;

        assert_eq!(result, ManagerEventSend::Queued);
        assert_eq!(consumer.await.expect("consumer task should finish"), 2);
    }

    #[tokio::test]
    async fn queue_manager_event_reports_closed_channel() {
        let (tx, rx) = mpsc::channel(1);
        drop(rx);

        let result = queue_manager_event("user", &tx, BotEvent::Kill, "Kill").await;

        assert_eq!(result, ManagerEventSend::Closed);
    }
}
