use rustc_hash::FxHasher;
use std::collections::{HashMap, HashSet};
use std::hash::BuildHasherDefault;

use log::info;

use kwant::indicators::Price;

use crate::strategy::{Strat, StratContext};
use crate::trade_setup::{TimeFrame, TradeParams};
use crate::{
    EngineOrder, ExecCommand, IndicatorData, MIN_ORDER_VALUE, MarketCommand, PositionOp, Side,
    Strategy, get_time_now,
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
}

unsafe impl Send for SignalEngine {}
impl SignalEngine {
    pub async fn new(
        config: Option<Vec<IndexId>>,
        trade_params: TradeParams,
        engine_rv: UnboundedReceiver<EngineCommand>,
        data_tx: Option<tokioSender<MarketCommand>>,
        trade_tx: Sender<ExecCommand>,
        exec_params: ExecParams,
    ) -> Self {
        let strategy = trade_params.strategy.init();
        let required_indicators = strategy.required_indicators();
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
            strategy,
            exec_params,
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

    fn get_signal(&mut self, price: f64, values: ValuesMap) -> Option<EngineOrder> {
        let ctx = StratContext {
            free_margin: self.exec_params.free_margin(),
            lev: self.exec_params.lev,
            last_price: price,
            indicators: &values,
            tick_time: get_time_now(),
            open_pos: self.exec_params.open_pos.as_ref(),
        };
        self.strategy.on_tick(ctx)
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

    fn validate_trade(&self, trade: &EngineOrder, last_price: f64) -> Result<(), String> {
        let side = match (trade.action, self.exec_params.open_pos) {
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

        if trade.action != PositionOp::Close
            && trade.size > self.exec_params.get_max_open_size(last_price)
        {
            return Err(
                "EXCEEDED MAX_SIZE: Trade size exceeded maximum available (free_margin * lev / last_price)".into()
            );
        }

        if let Some(limit) = trade.limit {
            validate_limit(&limit, side, last_price)?;
            if trade.size * limit.limit_px < MIN_ORDER_VALUE {
                return Err(format!(
                    "INVALID ORDER: notional value is below the minimum order value of {}$",
                    MIN_ORDER_VALUE
                ));
            }
        } else {
            match trade.action {
                PositionOp::OpenLong | PositionOp::OpenShort => {
                    if trade.size * last_price < MIN_ORDER_VALUE {
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
                        if trade.size < pos.size && trade.size * last_price > MIN_ORDER_VALUE {
                            return Err(format!(
                                "INVALID ORDER: notional value is below the minimum order value of {}$",
                                MIN_ORDER_VALUE
                            ));
                        }
                    } else {
                        return Err("INVALID STATE: Close order won't be processes, no open position present".to_string());
                    }
                }
            }
        }

        Ok(())
    }
}

impl SignalEngine {
    pub async fn start(&mut self) {
        while let Some(cmd) = self.engine_rv.recv().await {
            match cmd {
                EngineCommand::UpdatePrice(price) => {
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

                        if let Some(trade) = self.get_signal(price.close, values) {
                            if let Err(e) = self.validate_trade(&trade, price.close) {
                                eprintln!("\x1b[31m[{}]\x1b[0m", e);
                            } else {
                                //send to executor
                                let _ = self.trade_tx.try_send(ExecCommand::Order(trade));
                            }
                        }
                    }
                }

                EngineCommand::UpdatePriceBulk(data) => {
                    self.digest_bulk(data);
                }

                EngineCommand::UpdateStrategy(new_strat) => {
                    self.strategy = new_strat.init();
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
                            EditType::Toggle => self.toggle_indicator(entry.id),
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
