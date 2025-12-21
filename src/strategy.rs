#![allow(unused_variables)]
#![allow(unused_assignments)]

use crate::signal::{ExecParams, ValuesMap};
use crate::strats::*;
use crate::{EngineOrder, IndexId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Strategy {
    RsiEmaScalp,
    SrsiAdxScalp,
}

impl Strategy {
    pub fn indicators(&self) -> Vec<IndexId> {
        use Strategy as S;
        match self {
            S::RsiEmaScalp => RsiEmaStrategy::required_indicators_static(),
            S::SrsiAdxScalp => SrsiAdxScalp::required_indicators_static(),
        }
    }

    pub fn init(&self) -> Box<dyn Strat> {
        use Strategy as S;
        match self {
            S::RsiEmaScalp => Box::new(RsiEmaStrategy::init()),
            S::SrsiAdxScalp => Box::new(SrsiAdxScalp::init()),
        }
    }
}

pub trait Strat {
    fn on_tick(
        &mut self,
        snapshot: ValuesMap,
        price: f64,
        params: &ExecParams,
        now: u64,
    ) -> Option<EngineOrder>;
    fn required_indicators(&self) -> Vec<IndexId>;
}

pub trait NeedsIndicators {
    // must match required_indicators_static
    fn required_indicators_static() -> Vec<IndexId>;
}
