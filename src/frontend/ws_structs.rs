use crate::{
    AssetMargin, AssetPrice, IndexId, MarginAllocation, OpenPositionLocal, TradeInfo, TradeParams,
    Value,
};
use hyperliquid_rust_sdk::AssetMeta;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddMarketInfo {
    pub asset: String,
    pub margin_alloc: MarginAllocation,
    pub trade_params: TradeParams,
    pub config: Option<Vec<IndexId>>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketInfo {
    pub asset: String,
    pub lev: usize,
    pub price: f64,
    pub params: TradeParams,
    pub margin: f64,
    pub pnl: f64,
    pub is_paused: bool,
    pub indicators: Vec<IndicatorData>,
    pub position: Option<OpenPositionLocal>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndicatorData {
    pub id: IndexId,
    pub value: Option<Value>,
}

#[derive(Copy, Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EditMarketInfo {
    Lev(usize),
    Trade(TradeInfo),
    OpenPosition(Option<OpenPositionLocal>),
    Price(f64),
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdateFrontend {
    PreconfirmMarket(String),
    ConfirmMarket(MarketInfo),
    UpdateTotalMargin(f64),
    UpdateMarketMargin(AssetMargin),
    UpdateIndicatorValues {
        asset: String,
        data: Vec<IndicatorData>,
    },
    MarketInfoEdit((String, EditMarketInfo)),
    UserError(String),
    LoadSession((Vec<MarketInfo>, Vec<AssetMeta>)),
}
