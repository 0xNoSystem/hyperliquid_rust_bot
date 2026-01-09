use crate::{Limit, Side, TriggerKind};

const MIN_LIMIT_MULT: f64 = 0.05;
const MAX_LIMIT_MULT: f64 = 15.0;

pub(super) fn validate_tpsl(
    trigger: TriggerKind,
    side: Side,
    limit_px: f64,
    last_price: f64,
) -> Result<(), String> {
    match (side, trigger) {
        (Side::Short, TriggerKind::Tp) if limit_px <= last_price => Err(
            "TPSL ERROR (Long TP): TP must be strictly above last_price.\n\
                            Conditional orders must refer to future price movement.\n\
                            Remove TP/SL semantics and submit a non-conditional order."
                .into(),
        ),

        (Side::Short, TriggerKind::Sl) if limit_px >= last_price => Err(
            "TPSL ERROR (Long SL): SL must be strictly below last_price.\n\
                            Conditional orders must refer to future price movement.\n\
                            Remove TP/SL semantics and submit a non-conditional order."
                .into(),
        ),

        (Side::Long, TriggerKind::Tp) if limit_px >= last_price => Err(
            "TPSL ERROR (Short TP): TP must be strictly below last_price.\n\
                            Conditional orders must refer to future price movement.\n\
                            Remove TP/SL semantics and submit a non-conditional order."
                .into(),
        ),

        (Side::Long, TriggerKind::Sl) if limit_px <= last_price => Err(
            "TPSL ERROR (Short SL): SL must be strictly above last_price.\n\
                            Conditional orders must refer to future price movement.\n\
                            Remove TP/SL semantics and submit a non-conditional order."
                .into(),
        ),

        _ => Ok(()),
    }
}

pub(super) fn validate_limit(limit: &Limit, side: Side, last_price: f64) -> Result<(), String> {
    if let Some(trigger) = limit.is_tpsl() {
        validate_tpsl(trigger, side, limit.limit_px, last_price)?;
    }

    if limit.limit_px <= 0f64 {
        return Err("Invalid limit price: must be positive".into());
    }

    if limit.limit_px < (MIN_LIMIT_MULT * last_price)
        || limit.limit_px > (MAX_LIMIT_MULT * last_price)
    {
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
