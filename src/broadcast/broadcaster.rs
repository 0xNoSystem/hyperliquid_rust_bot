use super::CacheCmdIn;
use super::PriceData;
use crate::{
    Error, MAX_DISCONNECTION_WINDOW, TimeFrame, candles_snapshot, get_all_assets, get_time_now,
    parse_candle, subscribe_candles,
};
use hyperliquid_rust_sdk::{AssetMeta, BaseUrl, InfoClient, Message};
use rustc_hash::FxHasher;
use std::collections::HashMap;
use std::hash::BuildHasherDefault;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{
    Mutex, broadcast,
    mpsc::{Sender, UnboundedReceiver, UnboundedSender, unbounded_channel},
    oneshot,
};

const SUBSCRIPTION_ATTEMPTS_MAX: u32 = 5;

type FxMap<K, V> = HashMap<K, V, BuildHasherDefault<FxHasher>>;
pub type SubReply = Result<SubscriptionReply, Error>;

pub struct SubscriptionReply {
    pub px_receiver: broadcast::Receiver<PriceData>,
    pub meta: AssetMeta,
}

pub struct SubscribePayload {
    pub asset: String,
    pub reply: oneshot::Sender<SubReply>,
}

pub enum BroadcastCmd {
    Subscribe(SubscribePayload),
    Unsubscribe(String),
    SetSubId { asset: String, sub_id: u32 },
    CleanUp(String),
}

struct AssetFeed {
    tx: broadcast::Sender<PriceData>,
    sub_id: Option<u32>,
}

pub struct Broadcaster {
    url: BaseUrl,
    info_client: Arc<Mutex<InfoClient>>,
    cmd_tx: UnboundedSender<BroadcastCmd>,
    cmd_rx: UnboundedReceiver<BroadcastCmd>,
    channels: FxMap<String, AssetFeed>,
    cache_tx: Sender<CacheCmdIn>,
    universe: Vec<AssetMeta>,
}

impl Broadcaster {
    pub async fn new(
        url: BaseUrl,
        cache_tx: Sender<CacheCmdIn>,
    ) -> Result<(Self, UnboundedSender<BroadcastCmd>), Error> {
        let info_client = InfoClient::with_reconnect(None, Some(url)).await?;
        let universe = get_all_assets(&info_client).await?;
        let (cmd_tx, cmd_rx) = unbounded_channel::<BroadcastCmd>();
        Ok((
            Broadcaster {
                url,
                info_client: Arc::new(Mutex::new(info_client)),
                cmd_tx: cmd_tx.clone(),
                cmd_rx,
                channels: HashMap::default(),
                cache_tx,
                universe,
            },
            cmd_tx,
        ))
    }

    async fn add_sub(&mut self, sub_req: SubscribePayload) -> Result<(), Error> {
        let asset = sub_req.asset;

        let meta = match self.universe.iter().find(|a| a.name == asset) {
            Some(m) => m.clone(),
            None => {
                let _ = sub_req.reply.send(Err(Error::AssetNotFound));
                return Ok(());
            }
        };

        let rx = if let Some(feed) = self.channels.get(&asset) {
            feed.tx.subscribe()
        } else {
            let (tx, rx) = broadcast::channel::<PriceData>(256);
            self.channels.insert(
                asset.clone(),
                AssetFeed {
                    tx: tx.clone(),
                    sub_id: None,
                },
            );

            let info_client = self.info_client.clone();
            let cmd_tx = self.cmd_tx.clone();
            let asset = asset.clone();
            let url = self.url;

            tokio::spawn(async move {
                if let Err(e) = spawn_hl_feed(info_client, cmd_tx, asset.clone(), tx, url).await {
                    log::error!("HL feed for {} exited with error: {:?}", &asset, e);
                }
            });

            rx
        };

        let _ = sub_req.reply.send(Ok(SubscriptionReply {
            px_receiver: rx,
            meta,
        }));

        Ok(())
    }

    fn unsubscribe_from_feed(&mut self, asset: String) {
        if let Some(feed) = self.channels.remove(&asset) {
            let _ = self.cache_tx.try_send(CacheCmdIn::DropFeed(asset.clone()));
            if let Some(sub_id) = feed.sub_id {
                let client = self.info_client.clone();
                tokio::spawn(async move {
                    let mut client = client.lock().await;
                    if let Err(e) = client.unsubscribe(sub_id).await {
                        log::warn!(
                            "failed to unsubscribe {} (sub_id {}): {:?}",
                            &asset,
                            sub_id,
                            e
                        );
                    }
                });
            }
        }
    }

    #[inline]
    #[allow(dead_code)]
    fn is_feed_idle(&self, asset: &str) -> bool {
        self.channels
            .get(asset)
            .map(|feed| feed.tx.receiver_count() == 0)
            .unwrap_or(true)
    }

    #[inline]
    fn set_sub_id(&mut self, asset: &str, sub_id: u32) {
        if let Some(feed) = self.channels.get_mut(asset) {
            feed.sub_id = Some(sub_id);
        }
    }

    pub async fn start(&mut self) {
        while let Some(cmd) = self.cmd_rx.recv().await {
            match cmd {
                BroadcastCmd::Subscribe(payload) => {
                    if let Err(e) = self.add_sub(payload).await {
                        log::error!("failed to add subscription: {:?}", e);
                    }
                }
                BroadcastCmd::Unsubscribe(asset) => self.unsubscribe_from_feed(asset),
                BroadcastCmd::SetSubId { asset, sub_id } => self.set_sub_id(&asset, sub_id),
                BroadcastCmd::CleanUp(asset) => {
                    log::warn!("cleaning up dead feed for {}", &asset);
                    self.channels.remove(&asset);
                    let _ = self.cache_tx.try_send(CacheCmdIn::DropFeed(asset));
                }
            }
        }
    }
}

async fn spawn_hl_feed(
    info_client: Arc<Mutex<InfoClient>>,
    cmd_tx: UnboundedSender<BroadcastCmd>,
    asset: String,
    tx: broadcast::Sender<PriceData>,
    url: BaseUrl,
) -> Result<(), Error> {
    let mut attempts = 0;
    let (sub_id, mut price_rx) = loop {
        let result = {
            let mut client = info_client.lock().await;
            subscribe_candles(&mut client, asset.as_str()).await
        };

        match result {
            Ok(sub) => break sub,
            Err(e) => {
                attempts += 1;
                if attempts >= SUBSCRIPTION_ATTEMPTS_MAX {
                    return Err(e);
                }
                log::warn!(
                    "Failed to subscribe to {} (attempt {}/{}): {:?}",
                    &asset,
                    attempts,
                    SUBSCRIPTION_ATTEMPTS_MAX,
                    e
                );
            }
        }
    };

    let _ = cmd_tx.send(BroadcastCmd::SetSubId {
        asset: asset.clone(),
        sub_id,
    });

    let mut disconnected = false;
    let mut disconnection_start: Option<Instant> = None;
    let mut last_confirmed_close: Option<u64> = None;

    while let Some(msg) = price_rx.recv().await {
        match msg {
            Message::Candle(candle) => match parse_candle(candle.data) {
                Ok(price) => {
                    if disconnected {
                        disconnected = false;
                        if let Some(timer) = disconnection_start.take()
                            && timer.elapsed().as_millis() > MAX_DISCONNECTION_WINDOW
                        {
                            let disc_start = last_confirmed_close.unwrap_or_else(get_time_now);
                            let end = get_time_now();

                            let fetch_client = InfoClient::new(None, Some(url)).await;

                            if let Ok(client) = fetch_client {
                                match candles_snapshot(
                                    &client,
                                    &asset,
                                    TimeFrame::Min1,
                                    disc_start,
                                    end,
                                )
                                .await
                                {
                                    Ok(missed) if !missed.is_empty() => {
                                        log::info!(
                                            "recovered {} missed 1m candles for {}",
                                            missed.len(),
                                            &asset
                                        );
                                        let _ = tx.send(PriceData::Bulk(missed));
                                    }
                                    Ok(_) => {}
                                    Err(e) => {
                                        log::warn!(
                                            "failed to fetch missed window for {}: {:?}",
                                            &asset,
                                            e
                                        );
                                    }
                                }
                            }
                        }
                    }
                    last_confirmed_close = Some(price.open_time);
                    let _ = tx.send(PriceData::Single(price));
                }
                Err(e) => log::warn!("malformed candle for {}: {:?}", &asset, e),
            },
            Message::NoData => {
                if !disconnected {
                    disconnected = true;
                    disconnection_start = Some(Instant::now());
                    log::info!("{} price stream disconnected", &asset);
                }
            }
            _ => {}
        }
    }

    let _ = cmd_tx.send(BroadcastCmd::CleanUp(asset));
    Ok(())
}
