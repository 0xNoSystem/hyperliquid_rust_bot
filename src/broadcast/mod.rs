mod broadcaster;
mod candle_cache;
mod user_event_relay;

use std::sync::Arc;

use crate::Price;

pub use broadcaster::{BroadcastCmd, Broadcaster, SubReply, SubscribePayload, SubscriptionReply};
pub use candle_cache::{CacheCmdIn, CandleCache, CandleCount, CandleSnapshotRequest};
pub use user_event_relay::{
    QN_BUILD_ENDPOINTS_PER_ACCOUNT, QN_BUILD_MONTHLY_CREDITS, QN_BUILD_RPS_PER_ACCOUNT,
    QN_MAX_NAMED_FILTERS_PER_STREAM_TYPE, QN_MAX_TOTAL_FILTER_VALUES_PER_FILTER,
    QN_MAX_USER_VALUES_PER_FILTER, SUBSCRIBE_USER_EVENTS_CALLS_PER_WS,
    USERS_PER_BUILD_ACCOUNT_FOR_FILLS_AND_FUNDINGS, USERS_PER_SUBSCRIBE_USER_EVENTS_CALL,
    USERS_PER_WS_FOR_FILLS_AND_FUNDINGS, UserEventRelay, UserEventRelayHandle,
};

#[derive(Debug, Clone)]
pub enum PriceData {
    Single(Price),
    Bulk(Vec<Price>),
}

pub type PriceAsset = (Arc<str>, PriceData);
