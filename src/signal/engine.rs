#![allow(unused_variables)]
use rustc_hash::FxHasher;
use std::collections::{HashMap, HashSet};
use std::hash::BuildHasherDefault;

use log::info;
use serde::{Deserialize, Serialize};

use kwant::indicators::Price;

use crate::strategy::{Strat, StratContext};
use crate::trade_setup::TimeFrame;
use crate::{
    BusyType, EngineOrder, ExecCommand, ExecControl, IndicatorData, Intent, LiqSide,
    LiveTimeoutInfo, MIN_ORDER_VALUE, MarketCommand, OnTimeout, PositionOp, Side, Strategy,
    TimeoutInfo, TriggerKind, Triggers,
};

use flume::{Sender, bounded};
use tokio::sync::mpsc::{Sender as tokioSender, UnboundedReceiver, unbounded_channel};

use super::helpers::*;
use super::types::*;

type TrackersMap = HashMap<TimeFrame, Box<Tracker>, BuildHasherDefault<FxHasher>>;

pub struct SignalEngine {
    engine_rv: UnboundedReceiver<EngineCommand>,
    trade_tx: Sender<ExecCommand>,
    data_tx: Option<tokioSender<MarketCommand>>,
    trackers: TrackersMap,
    strategy: Box<dyn Strat>,
    exec_params: ExecParams,
    state: EngineState,
    pending_orders: Option<PendingOpen>,
}

impl SignalEngine {
    pub async fn new(
        config: Option<Vec<IndexId>>,
        strategy: Strategy,
        engine_rv: UnboundedReceiver<EngineCommand>,
        data_tx: Option<tokioSender<MarketCommand>>,
        trade_tx: Sender<ExecCommand>,
        exec_params: ExecParams,
    ) -> Self {
        let strategy_impl = strategy.init();
        let required_indicators = strategy_impl.required_indicators();
        let mut indicators: HashSet<IndexId> = if let Some(list) = config {
            list.into_iter().collect()
        } else {
            HashSet::new()
        };
        indicators.extend(required_indicators);

        let mut trackers: TrackersMap = HashMap::default();

        for id in indicators {
            if let Some(tracker) = &mut trackers.get_mut(&id.1) {
                tracker.add_indicator(id.0, false);
            } else {
                let mut new_tracker = Tracker::new(id.1);
                new_tracker.add_indicator(id.0, false);
                trackers.insert(id.1, Box::new(new_tracker));
            }
        }

        SignalEngine {
            engine_rv,
            trade_tx,
            data_tx,
            trackers,
            strategy: strategy_impl,
            exec_params,
            state: EngineState::Idle,
            pending_orders: None,
        }
    }

    pub fn reset(&mut self) {
        for tracker in self.trackers.values_mut() {
            tracker.reset();
        }
    }

    pub fn add_indicator(&mut self, id: IndexId) {
        if let Some(tracker) = &mut self.trackers.get_mut(&id.1) {
            tracker.add_indicator(id.0, true);
        } else {
            let mut new_tracker = Tracker::new(id.1);
            new_tracker.add_indicator(id.0, false);
            self.trackers.insert(id.1, Box::new(new_tracker));
        }
    }

    pub fn remove_indicator(&mut self, id: IndexId) {
        if let Some(tracker) = &mut self.trackers.get_mut(&id.1) {
            tracker.remove_indicator(id.0);
        }
    }

    pub fn toggle_indicator(&mut self, id: IndexId) {
        if let Some(tracker) = &mut self.trackers.get_mut(&id.1) {
            tracker.toggle_indicator(id.0);
        }
    }

    pub fn get_active_indicators(&self) -> Vec<IndexId> {
        let mut active = Vec::new();
        for (tf, tracker) in &self.trackers {
            for (kind, handler) in &tracker.indicators {
                if handler.is_active {
                    active.push((*kind, *tf));
                }
            }
        }
        active
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

    pub fn display_values(&self) {
        for (tf, tracker) in &self.trackers {
            for (kind, handler) in &tracker.indicators {
                if handler.is_active {
                    info!(
                        "\nKind: {:?} TF: {}\nValue: {:?}\n",
                        kind,
                        tf.as_str(),
                        handler.get_value()
                    );
                }
            }
        }
    }

    pub async fn load<I: IntoIterator<Item = Price>>(&mut self, tf: TimeFrame, price_data: I) {
        if let Some(tracker) = self.trackers.get_mut(&tf) {
            tracker.load(price_data);
        }
    }

    fn strat_tick(&mut self, price: Price, values: ValuesMap) -> Option<Intent> {
        use EngineState as E;
        let ctx = StratContext {
            free_margin: self.exec_params.free_margin(),
            lev: self.exec_params.lev,
            last_price: price,
            indicators: &values,
        };

        match self.state {
            E::Idle => self.strategy.on_idle(ctx, None),
            E::Armed(expiry) => self.strategy.on_idle(ctx, Some(expiry)),
            E::Opening(timeout) => self.strategy.on_busy(ctx, BusyType::Opening(timeout)),
            E::Closing(timeout) => self.strategy.on_busy(ctx, BusyType::Closing(timeout)),
            E::Open(open_pos) => self.strategy.on_open(ctx, &open_pos),
        }
    }

    fn digest(&mut self, price: Price) {
        for (_tf, tracker) in self.trackers.iter_mut() {
            tracker.digest(price);
        }
    }

    fn digest_bulk(&mut self, data: TimeFrameData) {
        for (tf, prices) in data.into_iter() {
            if let Some(tracker) = self.trackers.get_mut(&tf) {
                tracker.digest_bulk(prices);
            }
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

        let side = match (action, self.exec_params.open_pos) {
            (PositionOp::Close, None) => {
                return Err("INVALID STATE: Close with no open position".into());
            }
            (PositionOp::Close, Some(pos)) => !pos.side,

            (PositionOp::OpenLong, Some(pos)) if pos.side == Side::Short => {
                return Err("INVALID STATE: OpenLong while Short is open".into());
            }
            (PositionOp::OpenLong, _) => Side::Long,

            (PositionOp::OpenShort, Some(pos)) if pos.side == Side::Long => {
                return Err("INVALID STATE: OpenShort while Long is open".into());
            }
            (PositionOp::OpenShort, _) => Side::Short,
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
                    //MARKET CLOSE DOESN'T HAVE TO BE LESS THAN MIN_ORDER_VALUE IFF we're closing
                    //non-partial closes only
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

    pub fn force_as_taker_order(&self, intent: &Intent, last_price: &Price) -> Option<EngineOrder> {
        match intent {
            Intent::Open(order) => {
                let size = order.size.get_size(
                    self.exec_params.lev as f64,
                    self.exec_params.free_margin(),
                    last_price.close,
                );
                Some(EngineOrder::new_market_open(order.side, size))
            }

            Intent::Reduce(order) => {
                let size = order.size.get_size(
                    self.exec_params.lev as f64,
                    self.exec_params.free_margin(),
                    last_price.close,
                );
                Some(EngineOrder::market_close(size))
            }

            _ => None,
        }
    }

    #[inline]
    pub fn force_close_exec(&self) {
        let _ = self
            .trade_tx
            .send(ExecCommand::Control(ExecControl::ForceClose));
    }

    pub fn refresh_state(&mut self, price: &Price) {
        match self.state {
            //check for timeouts
            EngineState::Opening(timeout) => {
                if let Some(open_pos) = self.exec_params.open_pos {
                    self.state = EngineState::Open(open_pos);
                    return;
                }
                if timeout.expire_at <= price.open_time {
                    match timeout.timeout_info.action {
                        OnTimeout::Force => {
                            //translate to market order
                            if let Intent::Flatten(_) = timeout.intent {
                                self.force_close_exec();
                            } else if let Some(order) =
                                self.force_as_taker_order(&timeout.intent, price)
                            {
                                let _ = self.trade_tx.send(ExecCommand::Order(order));
                            }
                        }
                        OnTimeout::Cancel => {
                            self.force_close_exec();
                        }
                    }
                    self.state = EngineState::Idle;
                    let _ = self.pending_orders.take();
                }
            }

            EngineState::Closing(timeout) => {
                if self.exec_params.open_pos.is_none() {
                    self.state = EngineState::Idle;
                    return;
                }
                if timeout.expire_at <= price.open_time {
                    match timeout.timeout_info.action {
                        OnTimeout::Force => {
                            //translate to market order
                            if let Intent::Flatten(_) = timeout.intent {
                                self.force_close_exec();
                            } else if let Some(order) =
                                self.force_as_taker_order(&timeout.intent, price)
                            {
                                let _ = self.trade_tx.send(ExecCommand::Order(order));
                            }
                        }
                        OnTimeout::Cancel => {
                            self.force_close_exec();
                        }
                    }
                    self.state = EngineState::Idle;
                    let _ = self.pending_orders.take();
                }
            }

            EngineState::Armed(expire_at) => {
                if price.open_time >= expire_at {
                    self.state = EngineState::Idle;
                }
            }

            EngineState::Idle => {
                if let Some(open_pos) = self.exec_params.open_pos {
                    self.state = EngineState::Open(open_pos);
                }
            }

            EngineState::Open(_) => {
                if let Some(open_pos) = self.exec_params.open_pos {
                    self.state = EngineState::Open(open_pos);

                    if let Some(pending) = self.pending_orders.take()
                        && let Some(Triggers { tp, sl }) = pending.tpsl
                    {
                        let size = pending.open.size;
                        let side = open_pos.side;
                        let ref_px = open_pos.entry_px;

                        if let Some(tp_delta) = tp {
                            let trigger_px = calc_trigger_px(
                                side,
                                TriggerKind::Tp,
                                tp_delta,
                                ref_px,
                                self.exec_params.lev,
                            );
                            let _ = self
                                .trade_tx
                                .send(ExecCommand::Order(EngineOrder::new_tp(size, trigger_px)));
                        }

                        if let Some(sl_delta) = sl {
                            let trigger_px = calc_trigger_px(
                                side,
                                TriggerKind::Sl,
                                sl_delta,
                                ref_px,
                                self.exec_params.lev,
                            );
                            let _ = self
                                .trade_tx
                                .send(ExecCommand::Order(EngineOrder::new_sl(size, trigger_px)));
                        }
                    }
                } else {
                    self.state = EngineState::Idle;
                }
            }
        }
    }
}

impl SignalEngine {
    pub async fn start(&mut self) {
        while let Some(cmd) = self.engine_rv.recv().await {
            match cmd {
                EngineCommand::UpdatePrice(price) => {
                    let init_state = self.state;
                    self.digest(price);

                    let ind = self.get_indicators_data();
                    //let values: Vec<Value> = ind.iter().filter_map(|t| t.value).collect();
                    let values = self.get_active_values();

                    if !ind.is_empty()
                        && let Some(sender) = &self.data_tx
                    {
                        {
                            let _ = sender.send(MarketCommand::UpdateIndicatorData(ind)).await;
                        }

                        self.refresh_state(&price);

                        if let Some(intent) = self.strat_tick(price, values) {
                            let busy = matches!(
                                self.state,
                                EngineState::Opening(_) | EngineState::Closing(_)
                            );

                            if busy && intent != Intent::Abort {
                                log::warn!("Intent ignored while busy: {:?}", intent);
                                continue;
                            }

                            if intent == Intent::Abort {
                                self.force_close_exec();
                                let _ = self.pending_orders.take();
                                self.state = EngineState::Idle;
                            } else if let Intent::Arm(duration) = intent {
                                if self.state == EngineState::Idle {
                                    self.state =
                                        EngineState::Armed(price.open_time + duration.as_ms());
                                } else {
                                    log::warn!(
                                        "Intent::Arm failed, Engine is not in Idle state: {:?}",
                                        self.state
                                    );
                                }
                            } else if intent == Intent::Disarm {
                                if let EngineState::Armed(_exp) = self.state {
                                    self.state = EngineState::Idle;
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
                                    let _ = self.trade_tx.send(ExecCommand::Order(main_order));

                                    if let Some(ttl) = intent.get_ttl() {
                                        let timeout = LiveTimeoutInfo {
                                            expire_at: price.open_time + ttl.duration.as_ms(),
                                            timeout_info: ttl,
                                            intent,
                                        };
                                        match intent {
                                            Intent::Reduce(_) | Intent::Flatten(_) => {
                                                self.state = EngineState::Closing(timeout)
                                            }
                                            Intent::Open(_) => {
                                                self.state = EngineState::Opening(timeout)
                                            }
                                            _ => {}
                                        }
                                    } else {
                                        let ttl = TimeoutInfo::default();
                                        let timeout = LiveTimeoutInfo {
                                            expire_at: price.open_time + ttl.duration.as_ms(),
                                            timeout_info: ttl,
                                            intent,
                                        };
                                        match intent {
                                            Intent::Reduce(_) | Intent::Flatten(_) => {
                                                self.state = EngineState::Closing(timeout)
                                            }
                                            Intent::Open(_) => {
                                                self.state = EngineState::Opening(timeout)
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                        }
                    }

                    if init_state != self.state {
                        if let Some(sender) = &self.data_tx {
                            let _ = sender
                                .send(MarketCommand::EngineStateChange(self.state.into()))
                                .await;
                        }
                        info!("Engine transition: {:?} -->{:?}", init_state, self.state);
                    }
                }

                EngineCommand::UpdatePriceBulk(data) => {
                    self.digest_bulk(data);
                }

                EngineCommand::UpdateStrategy(new_strat) => {
                    self.strategy = new_strat.init();
                    self.state = EngineState::Idle;
                }

                EngineCommand::EditIndicators {
                    indicators,
                    price_data,
                } => {
                    for entry in indicators {
                        match entry.edit {
                            EditType::Add => {
                                self.add_indicator(entry.id);
                            }
                            EditType::Remove => {
                                self.remove_indicator(entry.id);
                            }
                        }
                    }
                    if let Some(data) = price_data {
                        for (tf, prices) in data {
                            let _ = self.load(tf, prices).await;
                        }
                    }

                    //Update frontend without waiting for next price update which makes indicators
                    //editing appear laggy
                    let ind = self.get_indicators_data();
                    if let Some(sender) = &self.data_tx {
                        let _ = sender.send(MarketCommand::UpdateIndicatorData(ind)).await;
                    }
                }

                EngineCommand::UpdateExecParams(param) => {
                    use ExecParam::*;
                    match param {
                        Margin(m) => {
                            self.exec_params.margin = m;
                        }
                        Lev(l) => {
                            self.exec_params.lev = l;
                        }

                        OpenPosition(pos) => {
                            self.exec_params.open_pos = pos;
                        }
                    }
                }

                EngineCommand::Stop => {
                    return;
                }
            }
        }
    }

    pub fn display_indicators(&mut self, price: f64) {
        info!("\nPrice => {}\n", price);
        //let vec = self.get_active_indicators();
        self.display_values();
        //Update
    }

    pub fn new_backtest(trade_params: ExecParams, strategy: Strategy) -> Self {
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

        //channels won't be used in backtesting, these are placeholders
        let (_tx, dummy_rv) = unbounded_channel::<EngineCommand>();
        let (dummy_tx, _rx) = bounded::<ExecCommand>(0);

        SignalEngine {
            engine_rv: dummy_rv,
            trade_tx: dummy_tx,
            data_tx: None,
            trackers,
            strategy,
            exec_params: trade_params,
            state: EngineState::Idle,
            pending_orders: None,
        }
    }
}

pub enum EngineCommand {
    UpdatePrice(Price),
    UpdatePriceBulk(TimeFrameData),
    UpdateStrategy(Strategy),
    EditIndicators {
        indicators: Vec<Entry>,
        price_data: Option<TimeFrameData>,
    },
    UpdateExecParams(ExecParam),
    Stop,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EngineState {
    Idle,
    Armed(u64), //expiry_time
    Open(OpenPosInfo),
    Opening(LiveTimeoutInfo),
    Closing(LiveTimeoutInfo),
}

impl From<EngineState> for EngineView {
    fn from(state: EngineState) -> Self {
        match state {
            EngineState::Idle => EngineView::Idle,
            EngineState::Armed(_) => EngineView::Armed,
            EngineState::Opening(_) => EngineView::Opening,
            EngineState::Closing(_) => EngineView::Closing,
            EngineState::Open(_) => EngineView::Open,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum EngineView {
    Idle,
    Armed,
    Opening,
    Closing,
    Open,
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
