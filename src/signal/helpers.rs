use crate::{Limit, Side, TriggerKind, Triggers};

const MIN_LIMIT_MULT: f64 = 0.05;
const MAX_LIMIT_MULT: f64 = 15.0;

pub(super) fn validate_tpsl(tpsl: &Triggers) -> Result<(), String> {
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

pub(super) fn validate_limit(limit: &Limit, ref_px: f64) -> Result<(), String> {
    if limit.limit_px <= 0f64 {
        return Err("Invalid limit price: must be positive".into());
    }

    if limit.limit_px < (MIN_LIMIT_MULT * ref_px) || limit.limit_px > (MAX_LIMIT_MULT * ref_px) {
        return Err("Unreasonable limit price".into());
    }

    Ok(())
}

pub(super) fn calc_trigger_px(
    side: Side,
    trigger: TriggerKind,
    delta: f64,
    ref_px: f64,
    lev: usize,
) -> f64 {
    if lev == 0 || ref_px <= 0.0 {
        // Let engine validation handle this properly
        return ref_px;
    }

    if delta < 0.0 {
        log::warn!(
            "calc_trigger_px called with negative delta: {} (will likely be rejected)",
            delta
        );
    }

    let price_delta = delta / lev as f64;

    match (side, trigger) {
        (Side::Long, TriggerKind::Tp) => ref_px * (1.0 + price_delta),
        (Side::Short, TriggerKind::Tp) => ref_px * (1.0 - price_delta),
        (Side::Long, TriggerKind::Sl) => ref_px * (1.0 - price_delta),
        (Side::Short, TriggerKind::Sl) => ref_px * (1.0 + price_delta),
    }
}
