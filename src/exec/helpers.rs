use hyperliquid_rust_sdk::{Error, ExchangeDataStatus, ExchangeResponseStatus};

pub(super) fn extract_order_status(
    res: ExchangeResponseStatus,
) -> Result<ExchangeDataStatus, Error> {
    let response = match res {
        ExchangeResponseStatus::Ok(exchange_response) => exchange_response,
        ExchangeResponseStatus::Err(e) => {
            return Err(Error::ExecutionFailure(e));
        }
    };

    let status = response
        .data
        .filter(|d| !d.statuses.is_empty())
        .and_then(|d| d.statuses.first().cloned())
        .ok_or_else(|| {
            Error::ExecutionFailure("Exchange Error: Couldn't fetch trade status".to_string())
        })?;

    Ok(status)
}
