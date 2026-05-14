use crate::{Limit, Side, TriggerKind, Triggers};

const MIN_LIMIT_MULT: f64 = 0.05;
const MAX_LIMIT_MULT: f64 = 15.0;

pub(super) fn validate_finite_positive(label: &str, value: f64) -> Result<(), String> {
    if !value.is_finite() || value <= 0.0 {
        return Err(format!("Invalid {label}: must be finite and positive"));
    }

    Ok(())
}

pub(super) fn validate_tpsl(tpsl: &Triggers) -> Result<(), String> {
    if let Some(tp) = tpsl.tp {
        validate_finite_positive("Trigger TP", tp)?;
    }

    if let Some(sl) = tpsl.sl {
        validate_finite_positive("Trigger SL", sl)?;
        if sl >= 100.0 {
            return Err(
                "Invalid Trigger: SL must be < 100 (cannot exceed full margin loss)".into(),
            );
        }
    }

    Ok(())
}

pub(super) fn validate_limit(limit: &Limit, ref_px: f64) -> Result<(), String> {
    validate_finite_positive("reference price", ref_px)?;
    validate_finite_positive("limit price", limit.limit_px)?;

    if limit.limit_px < (MIN_LIMIT_MULT * ref_px) || limit.limit_px > (MAX_LIMIT_MULT * ref_px) {
        return Err("Unreasonable limit price".into());
    }

    Ok(())
}

pub(super) fn validate_trigger_price(trigger: TriggerKind, price: f64) -> Result<(), String> {
    if !price.is_finite() || price <= 0.0 {
        return Err(format!(
            "Invalid {trigger:?} trigger price: must be finite and positive"
        ));
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

    let price_delta = (delta / lev as f64) / 100.0;

    match (side, trigger) {
        (Side::Long, TriggerKind::Tp) => ref_px * (1.0 + price_delta),
        (Side::Short, TriggerKind::Tp) => ref_px * (1.0 - price_delta),
        (Side::Long, TriggerKind::Sl) => ref_px * (1.0 - price_delta),
        (Side::Short, TriggerKind::Sl) => ref_px * (1.0 + price_delta),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ClientOrderLocal, Limit, Tif};

    #[test]
    fn validate_tpsl_rejects_non_finite_values() {
        assert!(
            validate_tpsl(&Triggers {
                tp: Some(f64::NAN),
                sl: None
            })
            .is_err()
        );
        assert!(
            validate_tpsl(&Triggers {
                tp: None,
                sl: Some(f64::INFINITY),
            })
            .is_err()
        );
    }

    #[test]
    fn validate_limit_rejects_non_finite_values() {
        let order_type = ClientOrderLocal::ClientLimit(Tif::Gtc);
        assert!(validate_limit(&Limit::new(f64::NAN, order_type), 100.0).is_err());
        assert!(validate_limit(&Limit::new(100.0, order_type), f64::INFINITY).is_err());
    }

    #[test]
    fn validate_trigger_price_rejects_non_finite_and_negative_values() {
        assert!(validate_trigger_price(TriggerKind::Tp, f64::INFINITY).is_err());
        assert!(validate_trigger_price(TriggerKind::Sl, -1.0).is_err());
        assert!(validate_trigger_price(TriggerKind::Tp, 101.0).is_ok());
    }
}
