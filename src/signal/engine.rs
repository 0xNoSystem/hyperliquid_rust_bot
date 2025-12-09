use rustc_hash::FxHasher;
use std::collections::HashMap;
use std::hash::BuildHasherDefault;

use log::info;

use kwant::indicators::{Price, Value};

use crate::strategy::Strategy;
use crate::trade_setup::{TimeFrame, TradeCommand, TradeParams};
use crate::{IndicatorData, MarketCommand};

use flume::{Sender, bounded};
use tokio::sync::mpsc::{Sender as tokioSender, UnboundedReceiver, unbounded_channel};

use super::types::{EditType, Entry, ExecParam, ExecParams, IndexId, TimeFrameData, Tracker};

type TrackersMap = HashMap<TimeFrame, Box<Tracker>, BuildHasherDefault<FxHasher>>;

pub struct SignalEngine {
    engine_rv: UnboundedReceiver<EngineCommand>,
    trade_tx: Sender<TradeCommand>,
    data_tx: Option<tokioSender<MarketCommand>>,
    trackers: TrackersMap,
    strategy: Strategy,
    exec_params: ExecParams,
}

impl SignalEngine {
    pub async fn new(
        config: Option<Vec<IndexId>>,
        trade_params: TradeParams,
        engine_rv: UnboundedReceiver<EngineCommand>,
        data_tx: Option<tokioSender<MarketCommand>>,
        trade_tx: Sender<TradeCommand>,
        margin: f64,
    ) -> Self {
        let mut trackers: TrackersMap = HashMap::default();
        trackers.insert(
            trade_params.time_frame,
            Box::new(Tracker::new(trade_params.time_frame)),
        );

        if let Some(list) = config
            && !list.is_empty()
        {
            for id in list {
                if let Some(tracker) = &mut trackers.get_mut(&id.1) {
                    tracker.add_indicator(id.0, false);
                } else {
                    let mut new_tracker = Tracker::new(id.1);
                    new_tracker.add_indicator(id.0, false);
                    trackers.insert(id.1, Box::new(new_tracker));
                }
            }
        };

        SignalEngine {
            engine_rv,
            trade_tx,
            data_tx,
            trackers,
            strategy: trade_params.strategy,
            exec_params: ExecParams::new(margin, trade_params.lev, trade_params.time_frame),
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
            println!("CREATING A NEW TRACKER");
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

    pub fn get_active_values(&self) -> Vec<Value> {
        let mut values = Vec::new();
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

    pub fn change_strategy(&mut self, strategy: Strategy) {
        self.strategy = strategy;
        info!("Strategy changed to: {:?}", self.strategy);
    }

    pub fn get_strategy(&self) -> &Strategy {
        &self.strategy
    }

    pub async fn load<I: IntoIterator<Item = Price>>(&mut self, tf: TimeFrame, price_data: I) {
        if let Some(tracker) = self.trackers.get_mut(&tf) {
            tracker.load(price_data).await;
        }
    }

    fn get_signal(&self, price: f64, values: Vec<Value>) -> Option<TradeCommand> {
        match self.strategy {
            Strategy::Custom(brr) => brr.generate_signal(values, price, self.exec_params),
        }
    }

    #[allow(unused)]
    fn get_test_trade(&self, price: f64) -> Option<TradeCommand> {
        match self.strategy {
            Strategy::Custom(brr) => brr.generate_test_trade(price, self.exec_params),
        }
    }

    fn digest(&mut self, price: Price) {
        for (_tf, tracker) in self.trackers.iter_mut() {
            tracker.digest(price);
        }
    }
}

impl SignalEngine {
    pub async fn start(&mut self) {
        let mut tick: u64 = 0;

        while let Some(cmd) = self.engine_rv.recv().await {
            match cmd {
                EngineCommand::UpdatePrice(price) => {
                    self.digest(price);

                    let ind = self.get_indicators_data();
                    let values: Vec<Value> = ind.iter().filter_map(|t| t.value).collect();

                    if !ind.is_empty() {
                        if tick.is_multiple_of(2)
                            && let Some(sender) = &self.data_tx
                        {
                            let _ = sender.send(MarketCommand::UpdateIndicatorData(ind)).await;
                        }

                        if let Some(trade) = self.get_test_trade(price.close) {
                            let _ = self.trade_tx.try_send(trade);
                        }
                    }
                    tick += 1;
                }

                EngineCommand::UpdateStrategy(new_strat) => {
                    self.change_strategy(new_strat);
                }

                EngineCommand::EditIndicators {
                    indicators,
                    price_data,
                } => {
                    info!(
                        "Received Indicator Edit Vec of length : {}",
                        indicators.len()
                    );

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
                    if !ind.is_empty()
                        && let Some(sender) = &self.data_tx
                    {
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
                        Tf(t) => {
                            self.exec_params.tf = t;
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

    pub fn new_backtest(
        trade_params: ExecParams,
        strategy: Strategy,
        config: Option<Vec<IndexId>>,
    ) -> Self {
        let mut trackers: TrackersMap = HashMap::default();

        if let Some(list) = config
            && !list.is_empty()
        {
            for id in list {
                if let Some(tracker) = &mut trackers.get_mut(&id.1) {
                    tracker.add_indicator(id.0, false);
                } else {
                    let mut new_tracker = Tracker::new(id.1);
                    new_tracker.add_indicator(id.0, false);
                    trackers.insert(id.1, Box::new(new_tracker));
                }
            }
        }

        //channels won't be used in backtesting, these are placeholders
        let (_tx, dummy_rv) = unbounded_channel::<EngineCommand>();
        let (dummy_tx, _rx) = bounded::<TradeCommand>(0);

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
    UpdateStrategy(Strategy),
    EditIndicators {
        indicators: Vec<Entry>,
        price_data: Option<TimeFrameData>,
    },
    UpdateExecParams(ExecParam),
    Stop,
}
