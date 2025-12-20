use std::collections::HashMap;
use std::fmt::Debug;

use rustc_hash::FxHasher;
use std::hash::BuildHasherDefault;

use arraydeque::{ArrayDeque, behavior::Wrapping};
use kwant::indicators::{
    Adx, Atr, Ema, EmaCross, Indicator, Price, Rsi, Sma, SmaRsi, StochasticRsi, Value,
};

use crate::{IndicatorData, MAX_HISTORY, Side, TimeFrame,};
use log::warn;

use serde::{Deserialize, Serialize};

#[derive(Debug, Copy, Clone)]
pub struct ExecParams {
    pub margin: f64,
    pub lev: usize,
    pub sz_decimals: u32,
    pub open_pos: Option<OpenPosInfo>,
}

#[derive(Debug, Copy, Clone)]
pub struct OpenPosInfo {
    pub side: Side,
    pub size: f64,
    pub entry_px: f64,
    pub open_time: u64,
}

impl ExecParams {
    pub fn new(margin: f64, lev: usize, sz_decimals: u32) -> Self {
        Self {
            margin,
            lev,
            sz_decimals,
            open_pos: None,
        }
    }

    pub fn free_margin(&self) -> f64 {
        if let Some(open) = self.open_pos {
            self.margin - ((open.entry_px * open.size) / self.lev as f64)
        } else {
            self.margin
        }
    }
}

pub enum ExecParam {
    Margin(f64),
    Lev(usize),
    OpenPosition(Option<OpenPosInfo>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum IndicatorKind {
    Rsi(u32),
    SmaOnRsi {
        periods: u32,
        smoothing_length: u32,
    },
    StochRsi {
        periods: u32,
        k_smoothing: Option<u32>,
        d_smoothing: Option<u32>,
    },
    Adx {
        periods: u32,
        di_length: u32,
    },
    Atr(u32),
    Ema(u32),
    EmaCross {
        short: u32,
        long: u32,
    },
    Sma(u32),
}

type IndicatorBuffer = Box<ArrayDeque<ArchivedValue, { MAX_HISTORY }, Wrapping>>;
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct ArchivedValue {
    pub time: u64,
    pub value: Value,
}
impl ArchivedValue {
    #[inline]
    fn new(time: u64, value: Value) -> Self {
        Self { time, value }
    }
}

#[derive(Debug)]
pub struct Handler {
    pub indicator: Box<dyn Indicator>,
    pub is_active: bool,
    pub closed: bool,
    pub history: IndicatorBuffer,
}

impl Handler {
    pub fn new(indicator: IndicatorKind) -> Handler {
        Handler {
            indicator: match_kind(indicator),
            is_active: true,
            closed: false,
            history: Box::new(ArrayDeque::default()),
        }
    }

    #[inline]
    fn toggle(&mut self) -> bool {
        self.is_active = !self.is_active;
        self.is_active
    }

    #[inline]
    pub fn update_before_close(&mut self, price: Price) {
        self.indicator.update_before_close(price);
        self.closed = false
    }

    pub fn update_after_close(&mut self, price: Price, prev_close: u64) {
        self.indicator.update_after_close(price);
        self.closed = true;
        if let Some(v) = self.indicator.get_last() {
            self.history.push_back(ArchivedValue::new(prev_close, v));
        }
    }

    #[inline]
    pub fn get_value(&self) -> Option<Value> {
        self.indicator.get_last()
    }

    pub fn load<'a, I: IntoIterator<Item = &'a Price>>(&mut self, price_data: I) {
        let data_vec: Vec<Price> = price_data.into_iter().copied().collect();
        self.load_slice(data_vec.as_slice());
    }

    pub fn load_slice(&mut self, price_data: &[Price]) {
        self.indicator.load(price_data);
    }

    pub fn reset(&mut self) {
        self.indicator.reset();
    }
}

pub type IndexId = (IndicatorKind, TimeFrame);

fn match_kind(kind: IndicatorKind) -> Box<dyn Indicator> {
    match kind {
        IndicatorKind::Rsi(periods) => Box::new(Rsi::new(periods, periods, None, None, None)),
        IndicatorKind::SmaOnRsi {
            periods,
            smoothing_length,
        } => Box::new(SmaRsi::new(periods, smoothing_length)),
        IndicatorKind::StochRsi {
            periods,
            k_smoothing,
            d_smoothing,
        } => Box::new(StochasticRsi::new(periods, k_smoothing, d_smoothing)),
        IndicatorKind::Adx { periods, di_length } => Box::new(Adx::new(periods, di_length)),
        IndicatorKind::Atr(periods) => Box::new(Atr::new(periods)),
        IndicatorKind::Ema(periods) => Box::new(Ema::new(periods)),
        IndicatorKind::EmaCross { short, long } => Box::new(EmaCross::new(short, long)),
        IndicatorKind::Sma(periods) => Box::new(Sma::new(periods)),
    }
}

type History = Box<ArrayDeque<Price, { MAX_HISTORY }, Wrapping>>;
#[derive(Debug)]
pub struct Tracker {
    pub price_data: History,
    pub indicators: HashMap<IndicatorKind, Handler, BuildHasherDefault<FxHasher>>,
    tf: TimeFrame,
    prev_close: Option<u64>,
    next_close: Option<u64>,
}

impl Tracker {
    pub fn new(tf: TimeFrame) -> Self {
        Tracker {
            price_data: Box::new(ArrayDeque::default()),
            indicators: HashMap::default(),
            tf,
            prev_close: None,
            next_close: None,
        }
    }
    pub fn digest(&mut self, price: Price) {
        let ts = price.close_time;
        let tf_ms = self.tf.to_millis();

        let mut next = match self.next_close {
            Some(n) => n,
            None => {
                self.update_indicators_before_close(price);
                self.next_close = Some(ts);
                return;
            }
        };
        if ts >= next {
            while ts >= next {
                self.prev_close = Some(next);
                next += tf_ms;
            }
            self.next_close = Some(next);
            self.price_data.push_back(price);
            self.update_indicators_after_close(price, self.prev_close.unwrap());
        } else {
            self.update_indicators_before_close(price);
        }
    }

    fn update_indicators_after_close(&mut self, price: Price, prev_close: u64) {
        for handler in &mut self.indicators.values_mut() {
            handler.update_after_close(price, prev_close);
        }
    }

    #[inline]
    fn update_indicators_before_close(&mut self, price: Price) {
        for handler in &mut self.indicators.values_mut() {
            handler.update_before_close(price);
        }
    }

    pub fn load<I: IntoIterator<Item = Price>>(&mut self, price_data: I) {
        let buffer: Vec<Price> = price_data.into_iter().collect();
        if buffer.is_empty() {
            warn!("LOAD BUFFER IS EMPTY!!!");
            return;
        }
        let last = buffer.last().unwrap();
        let slice = buffer.as_slice();

        for handler in self.indicators.values_mut() {
            handler.load_slice(slice);
        }

        let tf_ms = self.tf.to_millis();
        self.prev_close = Some((last.close_time / tf_ms) * tf_ms);
        self.next_close = Some(self.prev_close.unwrap() + tf_ms);

        self.price_data.extend(buffer);
    }
    pub fn add_indicator(&mut self, kind: IndicatorKind, load: bool) {
        let mut handler = Handler::new(kind);
        if load {
            handler.load(&*self.price_data);
        }
        self.indicators.insert(kind, handler);
    }

    pub fn remove_indicator(&mut self, kind: IndicatorKind) {
        self.indicators.remove(&kind);
    }

    pub fn toggle_indicator(&mut self, kind: IndicatorKind) {
        if let Some(handler) = self.indicators.get_mut(&kind) {
            let _ = handler.toggle();
        }
    }

    pub fn get_active_values(&self) -> ValuesMap {
        let mut values: ValuesMap = HashMap::default();
        for (kind, handler) in self.indicators.iter() {
            if let Some(value) = handler.get_value() {
                let tv = TimedValue {
                    value,
                    on_close: handler.closed,
                    ts: self.prev_close.unwrap_or(0),
                };
                values.insert((*kind, self.tf), tv);
            }
        }
        values
    }

    pub fn get_indicators_data(&self) -> Vec<IndicatorData> {
        let mut values = Vec::new();
        for (kind, handler) in self.indicators.iter() {
            values.push(IndicatorData {
                id: (*kind, self.tf),
                value: handler.get_value(),
            });
        }
        values
    }

    pub fn reset(&mut self) {
        self.price_data.clear();
        for handler in self.indicators.values_mut() {
            handler.reset();
        }
    }
}

pub type TimeFrameData = HashMap<TimeFrame, Vec<Price>, BuildHasherDefault<FxHasher>>;

#[derive(Copy, Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: IndexId,
    pub edit: EditType,
}

#[derive(Copy, Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum EditType {
    Toggle,
    Add,
    Remove,
}

pub type ValuesMap = HashMap<IndexId, TimedValue, BuildHasherDefault<FxHasher>>;

#[derive(Debug, Copy, Clone)]
pub struct TimedValue {
    pub value: Value,
    pub on_close: bool,
    pub ts: u64,
}
