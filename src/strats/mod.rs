pub(super) use crate::{
    Armed, BusyType, IndexId, IndicatorKind, Intent, NeedsIndicators, OnTimeout, OpenPosInfo, Side,
    SizeSpec, Strat, StratContext, TimeFrame, TimeoutInfo, TriggerKind, Triggers, Value, timedelta,
};

include!(concat!(env!("OUT_DIR"), "/strats_gen.rs"));
