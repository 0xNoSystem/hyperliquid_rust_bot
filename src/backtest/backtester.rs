use rustc_hash::FxHasher;
use std::collections::HashMap;
use std::hash::BuildHasherDefault;

use crate::signal::Tracker;
use crate::strategy::{Strat, StratContext};
use crate::{
    BusyType, EngineOrder, ExecParams, FillInfo, FillType, IndicatorData, Intent, LiqSide,
    LiveTimeoutInfo, Limit, MIN_ORDER_VALUE, OpenPosInfo, PositionOp, Price, Side, Strategy,
    TimeFrame, TimeoutInfo, TradeInfo, Triggers, ValuesMap,
};

type TrackersMap = HashMap<TimeFrame, Box<Tracker>, BuildHasherDefault<FxHasher>>;

pub struct Backtester {
    trackers: TrackersMap,
    strategy: Box<dyn Strat>,
    exec_params: ExecParams,
    state: BacktestState,
    pending_orders: Option<PendingOpen>,
    last_price: Option<Price>,
}

impl Backtester {
    pub fn new(margin: f64, lev: usize, strategy: Strategy) -> Self {
        let strategy = strategy.init();
        let required_indicators = strategy.required_indicators();

        let mut trackers: TrackersMap = HashMap::default();

        for id in required_indicators {
            if let Some(tracker) = &mut trackers.get_mut(&id.1) {
                tracker.add_indicator(id.0, false);
            } else {
                let mut new_tracker = Tracker::new(id.1);
                new_tracker.add_indicator(id.0, false);
                trackers.insert(id.1, Box::new(new_tracker));
            }
        }

        let exec_params = ExecParams::new(margin, lev);

        Backtester {
            trackers,
            strategy,
            exec_params,
            state: BacktestState::Idle,
            pending_orders: None,
            last_price: None,
        }
    }

    pub fn tick(&mut self, price: Price) -> Option<EngineOrder> {
        self.last_price = Some(price);
        self.digest(price);
        self.refresh_state(&price);

        let values = self.get_active_values();
        let mut emitted: Option<EngineOrder> = None;

        if let Some(intent) = self.strat_tick(price, values) {
            let busy = matches!(
                self.state,
                BacktestState::Opening(_) | BacktestState::Closing(_)
            );

            if busy && intent != Intent::Abort {
                log::warn!("Intent ignored while busy: {:?}", intent);
            }

            if intent == Intent::Abort {
                let _ = self.pending_orders.take();
                self.state = BacktestState::Idle;
            } else if let Intent::Arm(duration) = intent {
                if self.state == BacktestState::Idle {
                    self.state = BacktestState::Armed(price.open_time + duration.as_ms());
                } else {
                    log::warn!(
                        "Intent::Arm failed, Engine is not in Idle state: {:?}",
                        self.state
                    );
                }
            } else if intent == Intent::Disarm {
                if let BacktestState::Armed(_exp) = self.state {
                    self.state = BacktestState::Idle;
                } else {
                    log::warn!(
                        "Intent::Disarm failed, Engine is not Armed: {:?}",
                        self.state
                    );
                }
            } else if let Some(pending) = self.translate_intent(&intent, &price) {
                if let Err(e) = self.validate_trade(pending, price.close) {
                    log::warn!("Trade rejected: {}", e);
                } else {
                    let main_order = match pending {
                        PendingOrder::Open(p) => {
                            if p.has_trigger() {
                                self.pending_orders = Some(p);
                            }
                            p.open
                        }
                        PendingOrder::Close(p) => p,
                    };
                    emitted = Some(main_order);

                    if let Some(ttl) = intent.get_ttl() {
                        let timeout = LiveTimeoutInfo {
                            expire_at: price.open_time + ttl.duration.as_ms(),
                            timeout_info: ttl,
                            intent,
                        };
                        match intent {
                            Intent::Reduce(_) | Intent::Flatten(_) => {
                                self.state = BacktestState::Closing(Some(timeout))
                            }
                            Intent::Open(_) => self.state = BacktestState::Opening(Some(timeout)),
                            _ => {}
                        }
                    } else if intent.is_market_order() {
                        let ttl = TimeoutInfo::default();
                        let timeout = LiveTimeoutInfo {
                            expire_at: price.open_time + ttl.duration.as_ms(),
                            timeout_info: ttl,
                            intent,
                        };
                        match intent {
                            Intent::Reduce(_) | Intent::Flatten(_) => {
                                self.state = BacktestState::Closing(Some(timeout))
                            }
                            Intent::Open(_) => self.state = BacktestState::Opening(Some(timeout)),
                            _ => {}
                        }
                    } else {
                        match intent {
                            Intent::Reduce(_) | Intent::Flatten(_) => {
                                self.state = BacktestState::Closing(None)
                            }
                            Intent::Open(_) => self.state = BacktestState::Opening(None),
                            _ => {}
                        }
                    }
                }
            }
        }
        emitted
    }

    pub fn reset(&mut self) {
        for tracker in self.trackers.values_mut() {
            tracker.reset();
        }
        self.state = BacktestState::Idle;
        self.pending_orders = None;
    }

    pub fn load<I: IntoIterator<Item = Price>>(&mut self, tf: TimeFrame, price_data: I) {
        if let Some(tracker) = self.trackers.get_mut(&tf) {
            tracker.load(price_data);
        }
    }

    pub fn get_active_values(&self) -> ValuesMap {
        let mut values: ValuesMap = HashMap::default();
        for tracker in self.trackers.values() {
            values.extend(tracker.get_active_values());
        }
        values
    }

    pub fn get_indicators_data(&self) -> Vec<IndicatorData> {
        let mut values = Vec::new();
        for tracker in self.trackers.values() {
            values.extend(tracker.get_indicators_data());
        }
        values
    }

    pub fn exec_params(&self) -> &ExecParams {
        &self.exec_params
    }

    pub fn exec_params_mut(&mut self) -> &mut ExecParams {
        &mut self.exec_params
    }

    pub fn update_open_pos(&mut self, open_pos: Option<OpenPosInfo>) -> Option<TradeInfo> {
        let prev = self.exec_params.open_pos;
        self.exec_params.open_pos = open_pos;
        self.pending_orders = None;

        let trade = match (prev, self.exec_params.open_pos, self.last_price) {
            (Some(prev_pos), None, Some(price)) => {
                let pnl = match prev_pos.side {
                    Side::Long => (price.close - prev_pos.entry_px) * prev_pos.size,
                    Side::Short => (prev_pos.entry_px - price.close) * prev_pos.size,
                };
                Some(TradeInfo {
                    side: prev_pos.side,
                    size: prev_pos.size,
                    pnl,
                    fees: 0.0,
                    funding: 0.0,
                    open: FillInfo {
                        time: prev_pos.open_time,
                        price: prev_pos.entry_px,
                        fill_type: FillType::Market,
                    },
                    close: FillInfo {
                        time: price.open_time,
                        price: price.close,
                        fill_type: FillType::Market,
                    },
                })
            }
            _ => None,
        };

        match self.exec_params.open_pos {
            Some(pos) => {
                if matches!(self.state, BacktestState::Opening(_) | BacktestState::Idle) {
                    self.state = BacktestState::Open(pos);
                }
            }
            None => {
                if matches!(self.state, BacktestState::Closing(_) | BacktestState::Open(_)) {
                    self.state = BacktestState::Idle;
                }
            }
        }

        trade
    }

    fn strat_tick(&mut self, price: Price, values: ValuesMap) -> Option<Intent> {
        let ctx = StratContext {
            free_margin: self.exec_params.free_margin(),
            lev: self.exec_params.lev,
            last_price: price,
            indicators: &values,
        };

        match self.state {
            BacktestState::Idle => self.strategy.on_idle(ctx, None),
            BacktestState::Armed(expiry) => self.strategy.on_idle(ctx, Some(expiry)),
            BacktestState::Opening(timeout) => {
                self.strategy.on_busy(ctx, BusyType::Opening(timeout))
            }
            BacktestState::Closing(timeout) => {
                self.strategy.on_busy(ctx, BusyType::Closing(timeout))
            }
            BacktestState::Open(open_pos) => self.strategy.on_open(ctx, &open_pos),
        }
    }

    fn digest(&mut self, price: Price) {
        for (_tf, tracker) in self.trackers.iter_mut() {
            tracker.digest(price);
        }
    }

    fn translate_intent(&mut self, intent: &Intent, last_price: &Price) -> Option<PendingOrder> {
        use Intent as I;

        match intent {
            I::Open(order) => {
                let (size, open) = match &order.liq_side {
                    LiqSide::Taker => {
                        let size = order.size.get_size(
                            self.exec_params.lev as f64,
                            self.exec_params.free_margin(),
                            last_price.close,
                        );
                        (size, EngineOrder::new_market_open(order.side, size))
                    }
                    LiqSide::Maker(limit) => {
                        let size = order.size.get_size(
                            self.exec_params.lev as f64,
                            self.exec_params.free_margin(),
                            limit.limit_px,
                        );
                        (
                            size,
                            EngineOrder::new_limit_open(order.side, size, limit.limit_px, None),
                        )
                    }
                };

                let tpsl = if order.tp.is_some() || order.sl.is_some() {
                    Some(Triggers {
                        tp: order.tp,
                        sl: order.sl,
                    })
                } else {
                    None
                };

                Some(PendingOrder::Open(PendingOpen { open, tpsl }))
            }

            I::Reduce(reduce) => {
                let close = match &reduce.liq_side {
                    LiqSide::Taker => {
                        let size = reduce.size.get_size(
                            self.exec_params.lev as f64,
                            self.exec_params.free_margin(),
                            last_price.close,
                        );
                        EngineOrder::market_close(size)
                    }
                    LiqSide::Maker(limit) => {
                        let size = reduce.size.get_size(
                            self.exec_params.lev as f64,
                            self.exec_params.free_margin(),
                            limit.limit_px,
                        );
                        EngineOrder::new_limit_close(size, limit.limit_px, None)
                    }
                };
                Some(PendingOrder::Close(close))
            }

            I::Flatten(liq) => {
                let size = self.exec_params.open_pos?.size;
                let close = match liq {
                    LiqSide::Taker => EngineOrder::market_close(size),
                    LiqSide::Maker(limit) => {
                        EngineOrder::new_limit_close(size, limit.limit_px, None)
                    }
                };
                Some(PendingOrder::Close(close))
            }

            _ => None,
        }
    }

    fn validate_trade(&self, trade: PendingOrder, last_price: f64) -> Result<(), String> {
        let action = match trade {
            PendingOrder::Close(order) => order.action,
            PendingOrder::Open(open_order) => open_order.open.action,
        };

        match (action, self.exec_params.open_pos) {
            (PositionOp::Close, None) => {
                return Err("INVALID STATE: Close with no open position".into());
            }
            (PositionOp::Close, Some(_pos)) => {}

            (PositionOp::OpenLong, Some(pos)) if pos.side == Side::Short => {
                return Err("INVALID STATE: OpenLong while Short is open".into());
            }
            (PositionOp::OpenLong, _) => {}

            (PositionOp::OpenShort, Some(pos)) if pos.side == Side::Long => {
                return Err("INVALID STATE: OpenShort while Long is open".into());
            }
            (PositionOp::OpenShort, _) => {}
        };

        match trade {
            PendingOrder::Close(ref order) => {
                self.validate_engine_order(order, last_price)?;
            }
            PendingOrder::Open(ref order) => {
                if order.open.size > self.exec_params.get_max_open_size(last_price) {
                    return Err(
                        "EXCEEDED MAX_SIZE: Trade size exceeded maximum available (free_margin * lev / last_price)".into()
                    );
                }
                self.validate_engine_order(&order.open, last_price)?;
                if order.has_trigger() {
                    validate_tpsl(&order.tpsl.unwrap())?;
                }
            }
        }

        Ok(())
    }

    fn validate_engine_order(&self, order: &EngineOrder, ref_px: f64) -> Result<(), String> {
        if let Some(limit) = order.limit {
            validate_limit(&limit, ref_px)?;
            if order.size * limit.limit_px < MIN_ORDER_VALUE {
                return Err(format!(
                    "INVALID ORDER: notional value is below the minimum order value of {}$",
                    MIN_ORDER_VALUE
                ));
            }
        } else {
            match order.action {
                PositionOp::OpenLong | PositionOp::OpenShort => {
                    if order.size * ref_px < MIN_ORDER_VALUE {
                        return Err(format!(
                            "INVALID ORDER: notional value is below the minimum order value of {}$",
                            MIN_ORDER_VALUE
                        ));
                    }
                }

                PositionOp::Close => {
                    if let Some(pos) = self.exec_params.open_pos {
                        if order.size < pos.size && order.size * ref_px < MIN_ORDER_VALUE {
                            return Err(format!(
                                "INVALID ORDER: notional value is below the minimum order value of {}$",
                                MIN_ORDER_VALUE
                            ));
                        }
                    } else {
                        return Err(
                        "INVALID STATE: Close order won't be processed, no open position present"
                            .to_string(),
                    );
                    }
                }
            }
        }
        Ok(())
    }

    fn refresh_state(&mut self, price: &Price) {
        match self.state {
            BacktestState::Opening(ttl_option) => {
                if let Some(open_pos) = self.exec_params.open_pos {
                    self.state = BacktestState::Open(open_pos);
                    return;
                }
                if let Some(timeout) = ttl_option
                    && timeout.expire_at <= price.open_time
                {
                    self.state = BacktestState::Idle;
                    let _ = self.pending_orders.take();
                }
            }

            BacktestState::Closing(ttl_option) => {
                if self.exec_params.open_pos.is_none() {
                    self.state = BacktestState::Idle;
                    return;
                }

                if let Some(timeout) = ttl_option
                    && timeout.expire_at <= price.open_time
                {
                    self.state = BacktestState::Idle;
                    let _ = self.pending_orders.take();
                }
            }

            BacktestState::Armed(expire_at) => {
                if price.open_time >= expire_at {
                    self.state = BacktestState::Idle;
                }
            }

            BacktestState::Idle => {
                if let Some(open_pos) = self.exec_params.open_pos {
                    self.state = BacktestState::Open(open_pos);
                }
            }

            BacktestState::Open(_) => {
                if let Some(open_pos) = self.exec_params.open_pos {
                    self.state = BacktestState::Open(open_pos);
                    let _ = self.pending_orders.take();
                } else {
                    self.state = BacktestState::Idle;
                }
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum BacktestState {
    Idle,
    Armed(u64),
    Open(OpenPosInfo),
    Opening(Option<LiveTimeoutInfo>),
    Closing(Option<LiveTimeoutInfo>),
}

#[derive(Copy, Clone, Debug)]
enum PendingOrder {
    Open(PendingOpen),
    Close(EngineOrder),
}

#[derive(Copy, Clone, Debug)]
struct PendingOpen {
    open: EngineOrder,
    tpsl: Option<Triggers>,
}

impl PendingOpen {
    fn has_trigger(&self) -> bool {
        self.tpsl
            .as_ref()
            .map(|t| t.tp.is_some() || t.sl.is_some())
            .unwrap_or(false)
    }
}

fn validate_tpsl(tpsl: &Triggers) -> Result<(), String> {
    if let Some(tp) = tpsl.tp
        && tp <= 0.0
    {
        return Err("Invalid Trigger: TP must be positive".into());
    }

    if let Some(sl) = tpsl.sl {
        if sl <= 0.0 {
            return Err("Invalid Trigger: SL must be positive".into());
        }
        if sl >= 100.0 {
            return Err(
                "Invalid Trigger: SL must be < 100 (cannot exceed full margin loss)".into(),
            );
        }
    }

    Ok(())
}

fn validate_limit(limit: &Limit, ref_px: f64) -> Result<(), String> {
    const MIN_LIMIT_MULT: f64 = 0.05;
    const MAX_LIMIT_MULT: f64 = 15.0;

    if limit.limit_px <= 0f64 {
        return Err("Invalid limit price: must be positive".into());
    }

    if limit.limit_px < (MIN_LIMIT_MULT * ref_px) || limit.limit_px > (MAX_LIMIT_MULT * ref_px) {
        return Err("Unreasonable limit price".into());
    }

    Ok(())
}
