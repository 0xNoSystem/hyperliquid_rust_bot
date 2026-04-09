mod broadcaster;
mod candle_cache;

use std::sync::Arc;

use crate::Price;

pub use broadcaster::{BroadcastCmd, Broadcaster, SubReply, SubscribePayload, SubscriptionReply};
pub use candle_cache::{CacheCmdIn, CandleCache, CandleCount, CandleSnapshotRequest};

#[derive(Debug, Clone)]
pub enum PriceData {
    Single(Price),
    Bulk(Vec<Price>),
}

pub type PriceAsset = (Arc<str>, PriceData);
