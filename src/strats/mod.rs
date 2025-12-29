pub(super) use crate::{
    EngineOrder, IndexId, IndicatorKind, MIN_ORDER_VALUE, NeedsIndicators, Side, Strat,
    StratContext, TimeFrame, TriggerKind, Value, timedelta,
};

include!(concat!(env!("OUT_DIR"), "/strats_gen.rs"));
