use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;

#[derive(Clone, Copy, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeMetricsSnapshot {
    pub frontend_ws_dropped: u64,
    pub trade_persistence_dropped: u64,
    pub price_router_dropped: u64,
    pub market_price_route_dropped: u64,
    pub quicknode_account_queue_dropped: u64,
    pub sdk_account_queue_dropped: u64,
    pub sdk_candle_bridge_dropped: u64,
    pub stream_account_event_dropped: u64,
    pub user_event_relay_event_dropped: u64,
    pub user_event_relay_subscriber_dropped: u64,
    pub user_event_relay_lagged: u64,
    pub engine_exec_command_dropped: u64,
    pub signal_engine_price_dropped: u64,
    pub strategy_log_dropped: u64,
    pub candle_cache_price_dropped: u64,
    pub market_frontend_price_dropped: u64,
}

static FRONTEND_WS_DROPPED: AtomicU64 = AtomicU64::new(0);
static TRADE_PERSISTENCE_DROPPED: AtomicU64 = AtomicU64::new(0);
static PRICE_ROUTER_DROPPED: AtomicU64 = AtomicU64::new(0);
static MARKET_PRICE_ROUTE_DROPPED: AtomicU64 = AtomicU64::new(0);
static QUICKNODE_ACCOUNT_QUEUE_DROPPED: AtomicU64 = AtomicU64::new(0);
static SDK_ACCOUNT_QUEUE_DROPPED: AtomicU64 = AtomicU64::new(0);
static SDK_CANDLE_BRIDGE_DROPPED: AtomicU64 = AtomicU64::new(0);
static STREAM_ACCOUNT_EVENT_DROPPED: AtomicU64 = AtomicU64::new(0);
static USER_EVENT_RELAY_EVENT_DROPPED: AtomicU64 = AtomicU64::new(0);
static USER_EVENT_RELAY_SUBSCRIBER_DROPPED: AtomicU64 = AtomicU64::new(0);
static USER_EVENT_RELAY_LAGGED: AtomicU64 = AtomicU64::new(0);
static ENGINE_EXEC_COMMAND_DROPPED: AtomicU64 = AtomicU64::new(0);
static SIGNAL_ENGINE_PRICE_DROPPED: AtomicU64 = AtomicU64::new(0);
static STRATEGY_LOG_DROPPED: AtomicU64 = AtomicU64::new(0);
static CANDLE_CACHE_PRICE_DROPPED: AtomicU64 = AtomicU64::new(0);
static MARKET_FRONTEND_PRICE_DROPPED: AtomicU64 = AtomicU64::new(0);

#[inline]
fn inc(counter: &AtomicU64) {
    counter.fetch_add(1, Ordering::Relaxed);
}

#[inline]
fn add(counter: &AtomicU64, value: u64) {
    counter.fetch_add(value, Ordering::Relaxed);
}

#[inline]
pub(crate) fn inc_frontend_ws_dropped() {
    inc(&FRONTEND_WS_DROPPED);
}

#[inline]
pub(crate) fn inc_trade_persistence_dropped() {
    inc(&TRADE_PERSISTENCE_DROPPED);
}

#[inline]
pub(crate) fn inc_price_router_dropped() {
    inc(&PRICE_ROUTER_DROPPED);
}

#[inline]
pub(crate) fn inc_market_price_route_dropped() {
    inc(&MARKET_PRICE_ROUTE_DROPPED);
}

#[inline]
pub(crate) fn inc_quicknode_account_queue_dropped() {
    inc(&QUICKNODE_ACCOUNT_QUEUE_DROPPED);
}

#[inline]
pub(crate) fn inc_sdk_account_queue_dropped() {
    inc(&SDK_ACCOUNT_QUEUE_DROPPED);
}

#[inline]
pub(crate) fn inc_sdk_candle_bridge_dropped() {
    inc(&SDK_CANDLE_BRIDGE_DROPPED);
}

#[inline]
pub(crate) fn inc_stream_account_event_dropped() {
    inc(&STREAM_ACCOUNT_EVENT_DROPPED);
}

#[inline]
pub(crate) fn inc_user_event_relay_event_dropped() {
    inc(&USER_EVENT_RELAY_EVENT_DROPPED);
}

#[inline]
pub(crate) fn inc_user_event_relay_subscriber_dropped() {
    inc(&USER_EVENT_RELAY_SUBSCRIBER_DROPPED);
}

#[inline]
pub(crate) fn add_user_event_relay_lagged(value: u64) {
    add(&USER_EVENT_RELAY_LAGGED, value);
}

#[inline]
pub(crate) fn inc_engine_exec_command_dropped() {
    inc(&ENGINE_EXEC_COMMAND_DROPPED);
}

#[inline]
pub(crate) fn inc_signal_engine_price_dropped() {
    inc(&SIGNAL_ENGINE_PRICE_DROPPED);
}

#[inline]
pub(crate) fn inc_strategy_log_dropped() {
    inc(&STRATEGY_LOG_DROPPED);
}

#[inline]
pub(crate) fn inc_candle_cache_price_dropped() {
    inc(&CANDLE_CACHE_PRICE_DROPPED);
}

#[inline]
pub(crate) fn inc_market_frontend_price_dropped() {
    inc(&MARKET_FRONTEND_PRICE_DROPPED);
}

pub fn runtime_metrics_snapshot() -> RuntimeMetricsSnapshot {
    RuntimeMetricsSnapshot {
        frontend_ws_dropped: FRONTEND_WS_DROPPED.load(Ordering::Relaxed),
        trade_persistence_dropped: TRADE_PERSISTENCE_DROPPED.load(Ordering::Relaxed),
        price_router_dropped: PRICE_ROUTER_DROPPED.load(Ordering::Relaxed),
        market_price_route_dropped: MARKET_PRICE_ROUTE_DROPPED.load(Ordering::Relaxed),
        quicknode_account_queue_dropped: QUICKNODE_ACCOUNT_QUEUE_DROPPED.load(Ordering::Relaxed),
        sdk_account_queue_dropped: SDK_ACCOUNT_QUEUE_DROPPED.load(Ordering::Relaxed),
        sdk_candle_bridge_dropped: SDK_CANDLE_BRIDGE_DROPPED.load(Ordering::Relaxed),
        stream_account_event_dropped: STREAM_ACCOUNT_EVENT_DROPPED.load(Ordering::Relaxed),
        user_event_relay_event_dropped: USER_EVENT_RELAY_EVENT_DROPPED.load(Ordering::Relaxed),
        user_event_relay_subscriber_dropped: USER_EVENT_RELAY_SUBSCRIBER_DROPPED
            .load(Ordering::Relaxed),
        user_event_relay_lagged: USER_EVENT_RELAY_LAGGED.load(Ordering::Relaxed),
        engine_exec_command_dropped: ENGINE_EXEC_COMMAND_DROPPED.load(Ordering::Relaxed),
        signal_engine_price_dropped: SIGNAL_ENGINE_PRICE_DROPPED.load(Ordering::Relaxed),
        strategy_log_dropped: STRATEGY_LOG_DROPPED.load(Ordering::Relaxed),
        candle_cache_price_dropped: CANDLE_CACHE_PRICE_DROPPED.load(Ordering::Relaxed),
        market_frontend_price_dropped: MARKET_FRONTEND_PRICE_DROPPED.load(Ordering::Relaxed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_snapshot_exposes_incremented_counters() {
        let before = runtime_metrics_snapshot();

        inc_frontend_ws_dropped();
        inc_trade_persistence_dropped();
        inc_price_router_dropped();
        inc_market_price_route_dropped();
        inc_quicknode_account_queue_dropped();
        inc_sdk_account_queue_dropped();
        inc_sdk_candle_bridge_dropped();
        inc_stream_account_event_dropped();
        inc_user_event_relay_event_dropped();
        inc_user_event_relay_subscriber_dropped();
        add_user_event_relay_lagged(7);
        inc_engine_exec_command_dropped();
        inc_signal_engine_price_dropped();
        inc_strategy_log_dropped();
        inc_candle_cache_price_dropped();
        inc_market_frontend_price_dropped();

        let after = runtime_metrics_snapshot();

        assert!(after.frontend_ws_dropped > before.frontend_ws_dropped);
        assert!(after.trade_persistence_dropped > before.trade_persistence_dropped);
        assert!(after.price_router_dropped > before.price_router_dropped);
        assert!(after.market_price_route_dropped > before.market_price_route_dropped);
        assert!(after.quicknode_account_queue_dropped > before.quicknode_account_queue_dropped);
        assert!(after.sdk_account_queue_dropped > before.sdk_account_queue_dropped);
        assert!(after.sdk_candle_bridge_dropped > before.sdk_candle_bridge_dropped);
        assert!(after.stream_account_event_dropped > before.stream_account_event_dropped);
        assert!(after.user_event_relay_event_dropped > before.user_event_relay_event_dropped);
        assert!(
            after.user_event_relay_subscriber_dropped > before.user_event_relay_subscriber_dropped
        );
        assert!(after.user_event_relay_lagged >= before.user_event_relay_lagged + 7);
        assert!(after.engine_exec_command_dropped > before.engine_exec_command_dropped);
        assert!(after.signal_engine_price_dropped > before.signal_engine_price_dropped);
        assert!(after.strategy_log_dropped > before.strategy_log_dropped);
        assert!(after.candle_cache_price_dropped > before.candle_cache_price_dropped);
        assert!(after.market_frontend_price_dropped > before.market_frontend_price_dropped);
    }

    #[test]
    fn runtime_snapshot_serializes_with_camel_case_keys() {
        let value = serde_json::to_value(runtime_metrics_snapshot())
            .expect("metrics snapshot should serialize");

        assert!(value.get("frontendWsDropped").is_some());
        assert!(value.get("tradePersistenceDropped").is_some());
        assert!(value.get("userEventRelayLagged").is_some());
        assert!(value.get("engineExecCommandDropped").is_some());
        assert!(value.get("signalEnginePriceDropped").is_some());
        assert!(value.get("strategyLogDropped").is_some());
        assert!(value.get("candleCachePriceDropped").is_some());
        assert!(value.get("marketFrontendPriceDropped").is_some());
    }
}
