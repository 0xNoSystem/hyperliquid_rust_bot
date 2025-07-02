use serde::{Deserialize, Serialize};
use crate::{TradeInfo, MarginAllocation, IndexId, TradeParams, Value, AssetPrice, AssetMargin};

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddMarketInfo {
    pub asset: String,
    pub margin_alloc: MarginAllocation,
    pub trade_params: TradeParams,
    pub config: Option<Vec<IndexId>>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndicatorData{
    pub id: IndexId,
    pub value: Option<Value>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EditMarketInfo{
    Lev(f64),
    Strategy,
    Indicator(Vec<IndexId>),
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdateFrontend{
    UpdatePrice(AssetPrice),
    NewTradeInfo(TradeInfo),
    UpdateTotalMargin(f64),
    UpdateMarketMargin(AssetMargin),
    UpdateIndicatorValues{asset: String, data: Vec<IndicatorData>},
    MarketInfoEdit((String, EditMarketInfo)),
    UserError(String),
}
