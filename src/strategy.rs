#![allow(unused_variables)]
#![allow(unused_assignments)]

use crate::signal::ValuesMap;
use crate::{EngineOrder, IndexId, OpenPosInfo};

#[derive(Debug, Clone)]
pub struct StratContext<'a> {
    pub free_margin: f64,
    pub lev: usize,
    pub last_price: f64,
    pub indicators: &'a ValuesMap,
    pub tick_time: u64,
    pub open_pos: Option<&'a OpenPosInfo>,
}

pub trait Strat {
    fn on_tick(&mut self, ctx: StratContext) -> Option<EngineOrder>;
    fn required_indicators(&self) -> Vec<IndexId>;
}

pub trait NeedsIndicators {
    fn required_indicators_static() -> Vec<IndexId>;
}
