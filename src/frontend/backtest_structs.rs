use crate::backtest::{BacktestProgress, BacktestResult, BacktestRunRequest};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize)]
#[serde(transparent)]
pub struct BacktestRunPayload(pub BacktestRunRequest);

impl From<BacktestRunPayload> for BacktestRunRequest {
    fn from(value: BacktestRunPayload) -> Self {
        value.0
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BacktestRunResponse {
    pub run_id: String,
    pub result: BacktestResult,
    pub progress: Vec<BacktestProgress>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BacktestRunError {
    pub run_id: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub progress: Vec<BacktestProgress>,
}
