use crate::{
    AssetMargin, EngineView, IndexId, MarginAllocation, MarketState, OpenPositionLocal, Strategy,
    TradeInfo, Value,
};
use hyperliquid_rust_sdk::AssetMeta;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddMarketInfo {
    pub asset: String,
    pub margin_alloc: MarginAllocation,
    pub lev: usize,
    pub strategy: Strategy,
    pub config: Option<Vec<IndexId>>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketInfo {
    pub asset: String,
    pub lev: usize,
    pub strategy: Strategy,
    pub price: f64,
    pub margin: f64,
    pub pnl: f64,
    pub is_paused: bool,
    pub indicators: Vec<IndicatorData>,
    pub position: Option<OpenPositionLocal>,
    pub engine_state: EngineView,
}

impl From<&MarketState> for MarketInfo {
    fn from(s: &MarketState) -> Self {
        MarketInfo {
            asset: s.asset.clone(),
            lev: s.lev,
            price: 0.0,
            strategy: s.strategy,
            margin: s.margin,
            pnl: s.pnl,
            is_paused: s.is_paused,
            indicators: Vec::new(),
            position: s.position,
            engine_state: s.engine_state,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
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
    EngineState(EngineView),
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MarketStream {
    Price {
        asset: String,
        price: f64,
    },
    Indicators {
        asset: String,
        data: Vec<IndicatorData>,
    },
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdateFrontend {
    PreconfirmMarket(String),
    ConfirmMarket(MarketInfo),
    CancelMarket(String),
    UpdateTotalMargin(f64),
    UpdateMarketMargin(AssetMargin),
    MarketStream(MarketStream),
    MarketInfoEdit((String, EditMarketInfo)),
    UserError(String),
    LoadSession((Vec<MarketInfo>, Vec<AssetMeta>)),
    Status(BackendStatus),
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BackendStatus {
    Online,
    Offline,
    Shutdown,
}
