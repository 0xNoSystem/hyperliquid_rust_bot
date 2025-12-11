use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use alloy::signers::local::PrivateKeySigner;
use flume::Receiver;
use log::{info, warn};
use tokio::{
    sync::{Mutex, mpsc::Sender},
    time::{Duration, sleep},
};

use rustc_hash::FxHasher;
use std::hash::BuildHasherDefault;

use hyperliquid_rust_sdk::{
    BaseUrl, ClientCancelRequest, ClientLimit, ClientOrder, ClientOrderRequest, ClientTrigger,
    Error, ExchangeClient, ExchangeDataStatus, ExchangeResponseStatus, MarketOrderParams,
};

use crate::market::MarketCommand;
use crate::roundf;
use crate::trade_setup::{
    FillType, LimitOrderLocal, LimitOrderResponseLocal, OpenPositionLocal, TradeCommand,
    TradeFillInfo, TradeInfo, ClientOrderLocal,
};

pub struct Executor {
    trade_rv: Receiver<TradeCommand>,
    market_tx: Sender<MarketCommand>,
    asset: String,
    exchange_client: Arc<ExchangeClient>,
    is_paused: bool,
    fees: (f64, f64), //Maker, Taker
    resting_orders: HashMap<u64, LimitOrderLocal, BuildHasherDefault<FxHasher>>,
    open_position: Arc<Mutex<Option<OpenPositionLocal>>>,
}

impl Executor {
    pub async fn new(
        wallet: PrivateKeySigner,
        asset: String,
        fees: (f64, f64),
        trade_rv: Receiver<TradeCommand>,
        market_tx: Sender<MarketCommand>,
    ) -> Result<Executor, Error> {
        let exchange_client =
            Arc::new(ExchangeClient::new(None, wallet, Some(BaseUrl::Mainnet), None, None).await?);
        Ok(Executor {
            trade_rv,
            market_tx,
            asset,
            exchange_client,
            is_paused: false,
            fees,
            resting_orders: HashMap::default(),
            open_position: Arc::new(Mutex::new(None)),
        })
    }

    async fn try_market_trade(
        client: Arc<ExchangeClient>,
        params: MarketOrderParams<'_>,
    ) -> Result<ExchangeDataStatus, Error> {
        let response = client.market_open(params).await?;

        info!("Market order placed: {response:?}");

        let response = match response {
            ExchangeResponseStatus::Ok(exchange_response) => exchange_response,
            ExchangeResponseStatus::Err(e) => {
                return Err(Error::Custom(format!(
                    "Exchange Error: Couldn't execute trade => {}",
                    e
                )));
            }
        };

        let status = response
            .data
            .filter(|d| !d.statuses.is_empty())
            .and_then(|d| d.statuses.first().cloned())
            .ok_or_else(|| {
                Error::GenericRequest("Exchange Error: Couldn't fetch trade status".to_string())
            })?;

        Ok(status)
    }
    async fn market_open(&self, size: f64, is_long: bool) -> Result<TradeFillInfo, Error> {
        let market_open_params = MarketOrderParams {
            asset: self.asset.as_str(),
            is_buy: is_long,
            sz: size,
            px: None,
            slippage: Some(0.01), // 1%
            cloid: None,
            wallet: None,
        };

        let status =
            Self::try_market_trade(self.exchange_client.clone(), market_open_params).await?;

        match status {
            ExchangeDataStatus::Filled(ref order) => {
                println!("Open order filled: {order:?}");
                let sz: f64 = order.total_sz.parse::<f64>().map_err(|e| {
                    Error::GenericParse(format!("Failed to parse filled order size: {}", e))
                })?;
                let price: f64 = order.avg_px.parse::<f64>().map_err(|e| {
                    Error::GenericParse(format!("Failed to parse filled order price: {}", e))
                })?;
                let fee = sz * price * self.fees.1;
                let fill_info = TradeFillInfo {
                    fill_type: FillType::MarketOpen,
                    sz,
                    fee,
                    price,
                    oid: order.oid,
                    is_long,
                };

                Ok(fill_info)
            }

            _ => Err(Error::Custom("Market open order failed".to_string())),
        }
    }
    async fn market_close(&self, size: f64, is_long: bool) -> Result<TradeFillInfo, Error> {
        let market_close_params = MarketOrderParams {
            asset: self.asset.as_str(),
            is_buy: !is_long,
            sz: size,
            px: None,
            slippage: Some(0.01), // 1% slippage
            cloid: None,
            wallet: None,
        };

        let status =
            Self::try_market_trade(self.exchange_client.clone(), market_close_params).await?;
        match status {
            ExchangeDataStatus::Filled(ref order) => {
                println!("Close order filled: {order:?}");
                let sz: f64 = order.total_sz.parse::<f64>().map_err(|e| {
                    Error::GenericParse(format!("Failed to parse filled order size (close): {}", e))
                })?;
                let price: f64 = order.avg_px.parse::<f64>().map_err(|e| {
                    Error::GenericParse(format!(
                        "Failed to parse filled order price (close): {}",
                        e
                    ))
                })?;
                let fee = sz * price * self.fees.1;
                let fill_info = TradeFillInfo {
                    fill_type: FillType::MarketClose,
                    sz,
                    fee,
                    price,
                    oid: order.oid,
                    is_long,
                };
                Ok(fill_info)
            }

            _ => Err(Error::Custom("Close market order not filled".to_string())),
        }
    }

    async fn market_close_static(
        client: Arc<ExchangeClient>,
        asset: String,
        size: f64,
        is_long: bool,
        taker_fee: &f64,
    ) -> Result<TradeFillInfo, Error> {
        let market_close_params = MarketOrderParams {
            asset: asset.as_str(),
            is_buy: !is_long,
            sz: size,
            px: None,
            slippage: Some(0.01), // 1% slippage
            cloid: None,
            wallet: None,
        };

        let status = Self::try_market_trade(client, market_close_params).await?;
        match status {
            ExchangeDataStatus::Filled(ref order) => {
                println!("Close order filled: {order:?}");
                let sz: f64 = order.total_sz.parse::<f64>().map_err(|e| {
                    Error::GenericParse(format!("Failed to parse filled order size (close): {}", e))
                })?;
                let price: f64 = order.avg_px.parse::<f64>().map_err(|e| {
                    Error::GenericParse(format!(
                        "Failed to parse filled order price (close): {}",
                        e
                    ))
                })?;
                let fee = sz * price * taker_fee;
                let fill_info = TradeFillInfo {
                    fill_type: FillType::MarketClose,
                    sz,
                    fee,
                    price,
                    oid: order.oid,
                    is_long,
                };
                Ok(fill_info)
            }

            _ => Err(Error::Custom("Close market order not filled".to_string())),
        }
    }

    async fn try_limit_trade(
        client: Arc<ExchangeClient>,
        params: ClientOrderRequest,
    ) -> Result<ExchangeDataStatus, Error> {
        let response = client.order(params, None).await?;

        info!("Market order placed: {response:?}");

        let response = match response {
            ExchangeResponseStatus::Ok(exchange_response) => exchange_response,
            ExchangeResponseStatus::Err(e) => {
                return Err(Error::Custom(format!(
                    "Exchange Error: Couldn't execute limit trade => {}",
                    e
                )));
            }
        };

        let status = response
            .data
            .filter(|d| !d.statuses.is_empty())
            .and_then(|d| d.statuses.first().cloned())
            .ok_or_else(|| {
                Error::GenericRequest("Exchange Error: Couldn't fetch trade status".to_string())
            })?;

        Ok(status)
    }
    async fn limit_open(
        &self,
        limit_order: LimitOrderLocal,
    ) -> Result<LimitOrderResponseLocal, Error> {
        
        let order = ClientOrderRequest {
            asset: self.asset.clone(),
            is_buy: limit_order.is_long,
            reduce_only: false,
            limit_px: limit_order.limit_px,
            sz: limit_order.size,
            cloid: None,
            order_type: limit_order.client_order(),
        };

        let status = Self::try_limit_trade(self.exchange_client.clone(), order).await?;

        match status {
            ExchangeDataStatus::Filled(ref order) => {
                println!("Limit Open order filled as Taker: {order:?}");
                let sz: f64 = order.total_sz.parse::<f64>().map_err(|e| {
                    Error::GenericParse(format!("Failed to parse filled order size: {}", e))
                })?;
                let price: f64 = order.avg_px.parse::<f64>().map_err(|e| {
                    Error::GenericParse(format!("Failed to parse filled order price: {}", e))
                })?;
                let fee = sz * price * self.fees.1;
                Ok(LimitOrderResponseLocal::Filled(TradeFillInfo {
                    fill_type: FillType::LimitOpen,
                    sz,
                    fee,
                    price,
                    oid: order.oid,
                    is_long: limit_order.is_long,
                }))
            }

            ExchangeDataStatus::Resting(order) => Ok(LimitOrderResponseLocal::Resting(order)),

            ExchangeDataStatus::Error(err) => Err(Error::Custom(err)),

            _ => Err(Error::Custom(
                "Limit open order failed due to an unexpected exchange status response".to_string(),
            )),
        }
    }
    async fn limit_close(
        &self,
        limit_order: LimitOrderLocal,
    ) -> Result<LimitOrderResponseLocal, Error> {

        let order_type = limit_order.client_order();
        let order = ClientOrderRequest {
            asset: self.asset.clone(),
            is_buy: !limit_order.is_long,
            reduce_only: true,
            limit_px: limit_order.limit_px,
            sz: limit_order.size,
            cloid: None,
            order_type,
        };

        let fill_type: FillType = match limit_order.order_type{
            ClientOrderLocal::ClientLimit(_) => FillType::LimitClose,
            ClientOrderLocal::ClientTrigger(order) => FillType::Trigger(order.kind),
        };

        let status = Self::try_limit_trade(self.exchange_client.clone(), order).await?;

        match status {
            ExchangeDataStatus::Filled(ref order) => {
                println!("Limit Close order filled as Taker: {order:?}");
                let sz: f64 = order.total_sz.parse::<f64>().map_err(|e| {
                    Error::GenericParse(format!("Failed to parse filled order size: {}", e))
                })?;
                let price: f64 = order.avg_px.parse::<f64>().map_err(|e| {
                    Error::GenericParse(format!("Failed to parse filled order price: {}", e))
                })?;
                let fee = sz * price * self.fees.1;
                let fill_info = TradeFillInfo {
                    fill_type,
                    sz,
                    fee,
                    price,
                    oid: order.oid,
                    is_long: !limit_order.is_long,
                };

                Ok(LimitOrderResponseLocal::Filled(fill_info))
            }

            ExchangeDataStatus::Resting(order) => Ok(LimitOrderResponseLocal::Resting(order)),

            ExchangeDataStatus::Error(err) => Err(Error::Custom(err)),

            _ => Err(Error::Custom(
                "Limit open order failed due to an unexpected exchange status response".to_string(),
            )),
        }
    }

    async fn cancel_trade(&mut self) -> Option<TradeInfo> {
        if let Some(mut pos) = self.open_position.lock().await.take() {
            let trade_fill = self.market_close(pos.size, pos.is_long).await;
            if let Ok(close) = trade_fill {
                if let Some(trade_info) = pos.apply_close_fill(&close) {
                    return Some(trade_info);
                }
            }
            warn!("Failed to cancel trade for {} market", &self.asset);
        }
        None
    }

    async fn cancel_all_resting(&mut self) -> Result<(), Error> {
        let mut failed_cancels: HashSet<u64> = HashSet::new();
        for oid in self.resting_orders.keys() {
            let cancel = ClientCancelRequest {
                asset: self.asset.clone(),
                oid: *oid,
            };
            if let Err(e) = self.exchange_client.cancel(cancel, None).await {
                warn!("Failed to cancel oid {}: {:?}", oid, e);
                failed_cancels.insert(*oid);
            }
        }
        let mut retries = 0;

        while !failed_cancels.is_empty() {
            retries += 1;
            let iterator = failed_cancels.iter().copied().collect::<Vec<_>>();
            for oid in iterator.iter() {
                let cancel = ClientCancelRequest {
                    asset: self.asset.clone(),
                    oid: *oid,
                };
                if self.exchange_client.cancel(cancel, None).await.is_ok() {
                    failed_cancels.remove(oid);
                }
            }

            if retries > 5 {
                return Err(Error::Custom(format!(
                    "Failed to cancle resting order for {} market, please cancel manually on https://app.hyperliquid.xyz/trade/{}",
                    self.asset.clone(),
                    self.asset.clone()
                )));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Ok(())
    }

    async fn is_active(&self) -> bool {
        let guard = self.open_position.lock().await;
        guard.is_some() || !self.resting_orders.is_empty()
    }

    fn toggle_pause(&mut self) {
        self.is_paused = !self.is_paused
    }

    pub async fn handle_close_fill(&mut self, oid: u64, fill: TradeFillInfo) -> Option<TradeInfo> {
        let mut pos_guard = self.open_position.lock().await;

        if let (Some(open_pos), Some(resting_order)) =
            (&mut *pos_guard, self.resting_orders.get_mut(&oid))
        {
            assert_ne!(fill.is_long, resting_order.is_long);

            if fill.is_long {
                assert!(fill.price >= resting_order.limit_px);
            } else {
                assert!(fill.price <= resting_order.limit_px);
            }

            let trade = open_pos.apply_close_fill(&fill);
            resting_order.size -= fill.sz;
            if roundf!(resting_order.size, 6) == 0.0 {
                return trade;
            } else {
                return None;
            }
        }

        None
    }

    pub async fn handle_open_fill(&mut self, oid: u64, fill: TradeFillInfo) {
        let mut clean_up = false;
        let mut pos_guard = self.open_position.lock().await;

        if let Some(resting_order) = self.resting_orders.get_mut(&oid) {
            assert_eq!(fill.is_long, resting_order.is_long);

            if fill.is_long {
                assert!(fill.price <= resting_order.limit_px);
            } else {
                assert!(fill.price >= resting_order.limit_px);
            }

            resting_order.size -= fill.sz;

            if roundf!(resting_order.size, 6) == 0.0 {
                clean_up = true;
            }

            if let Some(pos) = &mut *pos_guard {
                pos.apply_open_fill(&fill);
            } else {
                *pos_guard = Some(OpenPositionLocal::new(self.asset.clone(), fill));
            }
        }

        if clean_up {
            self.resting_orders.remove(&oid);
        }
    }

    pub async fn start(&mut self) {

        let info_sender = self.market_tx.clone();
        while let Ok(cmd) = self.trade_rv.recv_async().await {
            match cmd {
                TradeCommand::ExecuteTrade {
                    size,
                    is_long,
                    duration,
                } => {
                    if self.is_active().await || self.is_paused {
                        continue;
                    };
                    let trade_info = self.market_open(size, is_long).await;
                    if let Ok(fill) = trade_info {
                        {
                            let pos_guard = &mut *self.open_position.lock().await;
                            if let Some(open_pos) = pos_guard {
                                open_pos.apply_open_fill(&fill);
                            } else {
                                *pos_guard = Some(OpenPositionLocal::new(self.asset.clone(), fill));
                            }
                        }

                        let client = self.exchange_client.clone();
                        let asset = self.asset.clone();
                        let fees = self.fees;
                        let sender = info_sender.clone();
                        let pos_handle = self.open_position.clone();
                        tokio::spawn(async move {
                            let _ = sleep(Duration::from_secs(duration)).await;
                            let mut maybe_open = {
                                let mut pos = pos_handle.lock().await;
                                pos.take()
                            };

                            if let Some(ref mut open) = maybe_open {
                                let close_fill = Self::market_close_static(
                                    client, asset, open.size, is_long, &fees.1,
                                )
                                .await;
                                if let Ok(fill) = close_fill {
                                    if let Some(trade_info) = open.apply_close_fill(&fill) {
                                        let _ = sender
                                            .send(MarketCommand::ReceiveTrade(trade_info))
                                            .await;
                                        info!("Trade Closed: {:?}", trade_info);
                                    }
                                }
                            }
                        });
                    };
                }

                TradeCommand::OpenTrade { size, is_long } => {
                    info!("Open trade command received");

                    if !self.is_active().await && !self.is_paused {
                        let trade_fill = self.market_open(size, is_long).await;

                        if let Ok(fill) = trade_fill {
                            let pos_guard = &mut *self.open_position.lock().await;
                            if let Some(open_pos) = pos_guard {
                                open_pos.apply_open_fill(&fill);
                            } else {
                                *pos_guard = Some(OpenPositionLocal::new(self.asset.clone(), fill));
                            }
                        };
                    } else if self.is_active().await {
                        info!("OpenTrade skipped: a trade is already active");
                    }
                }

                TradeCommand::CloseTrade { size } => {
                    if self.is_paused {
                        assert!(!self.is_active().await);
                        continue;
                    };
                    let maybe_open = {
                        let mut pos = self.open_position.lock().await;
                        pos.take()
                    };

                    if let Some(mut open_pos) = maybe_open {
                        let size = size.min(open_pos.size);
                        let trade_fill = self.market_close(size, open_pos.is_long).await;
                        if let Ok(fill) = trade_fill {
                            if let Some(trade_info) = open_pos.apply_close_fill(&fill) {
                                let _ = info_sender
                                    .send(MarketCommand::ReceiveTrade(trade_info))
                                    .await;
                                info!("Trade Closed: {:?}", trade_info);
                                let _ = self.cancel_all_resting().await;
                            }
                        }
                    };
                }

                TradeCommand::CancelTrade => {
                    if let Some(trade_info) = self.cancel_trade().await {
                        let _ = info_sender
                            .send(MarketCommand::ReceiveTrade(trade_info))
                            .await;
                    };
                    let _ = self.cancel_all_resting().await;

                    return;
                }

                TradeCommand::UserFills(fill) => {
                    use FillType::*;
                    let fill_type = fill.fill_type;
                    let oid = fill.oid;
                    match fill_type {
                        MarketClose => {
                            if let Some(trade_info) = self.handle_close_fill(oid, fill).await {
                                let _ = info_sender
                                    .send(MarketCommand::ReceiveTrade(trade_info))
                                    .await;
                                info!("Trade Closed: {:?}", trade_info);
                                let _ = self.cancel_all_resting().await;
                            }
                        }
                        MarketOpen => {
                            self.handle_open_fill(oid, fill).await;
                        }
                        LimitOpen => {
                            self.handle_open_fill(oid, fill).await;
                        }
                        LimitClose => {
                            if let Some(trade_info) = self.handle_close_fill(oid, fill).await {
                                let _ = info_sender
                                    .send(MarketCommand::ReceiveTrade(trade_info))
                                    .await;
                                info!("Trade Closed: {:?}", trade_info);
                                let _ = self.cancel_all_resting().await;
                            }
                        }
                        Trigger(_tpsl) => {
                            if let Some(trade_info) = self.handle_close_fill(oid, fill).await {
                                let _ = info_sender
                                    .send(MarketCommand::ReceiveTrade(trade_info))
                                    .await;
                                info!("Trade Closed: {:?}", trade_info);
                                let _ = self.cancel_all_resting().await;
                            }
                        }
                        Liquidation => {
                            if let Some(trade_info) = self.handle_close_fill(oid, fill).await {
                                let _ = info_sender
                                    .send(MarketCommand::ReceiveTrade(trade_info))
                                    .await;
                                info!("Trade Closed: {:?}", trade_info);
                                let _ = self.cancel_all_resting().await;
                            }
                        }
                        Mixed => {
                            warn!("MIXED FILL_TYPE CHECK LOG TO DEBUG");
                        }
                    }
                }

                TradeCommand::Toggle => {
                    if let Some(trade_info) = self.cancel_trade().await {
                        let _ = info_sender
                            .send(MarketCommand::ReceiveTrade(trade_info))
                            .await;
                    };
                    self.toggle_pause();
                    info!(
                        "Executor is now {}",
                        if self.is_paused { "paused" } else { "resumed" }
                    );
                }

                TradeCommand::Pause => {
                    if let Some(trade_info) = self.cancel_trade().await {
                        let _ = info_sender
                            .send(MarketCommand::ReceiveTrade(trade_info))
                            .await;
                        let _ = self.cancel_all_resting().await;
                    };
                    self.is_paused = true;
                }
                TradeCommand::Resume => {
                    self.is_paused = false;
                }

                TradeCommand::LimitOpen(limit_order) => {
                    println!("{:?}", &limit_order);
                    if !self.is_active().await && !self.is_paused {
                        match self.limit_open(limit_order).await {
                            Ok(fill_type) => {
                                match fill_type {
                                    LimitOrderResponseLocal::Filled(fill) => {
                                        //filled as taker
                                        let pos_guard = &mut *self.open_position.lock().await;
                                        if let Some(open_pos) = pos_guard {
                                            open_pos.apply_open_fill(&fill);
                                        } else {
                                            *pos_guard = Some(OpenPositionLocal::new(
                                                self.asset.clone(),
                                                fill,
                                            ));
                                        }
                                    }
                                    LimitOrderResponseLocal::Resting(oid) => {
                                        self.resting_orders.insert(oid.oid, limit_order.clone());
                                    }
                                }
                            }

                            Err(e) => {
                                warn!("{}", e);
                            }
                        }
                    } else if self.is_active().await {
                        info!("OpenTrade skipped: a trade is already active");
                    }
                }

                TradeCommand::LimitClose(mut limit_order) => {
                    println!("{:?}", &limit_order);
                    if self.is_paused {
                        assert!(!self.is_active().await);
                        continue;
                    };

                    let mut maybe_open = self.open_position.lock().await;
                    let mut clean_up = false;

                    if let Some(open_pos) = &mut *maybe_open {
                        limit_order.size = limit_order.size.min(open_pos.size);
                        let res = self.limit_close(limit_order).await;
                        if let Ok(fill_type) = res {
                            match fill_type {
                                LimitOrderResponseLocal::Filled(fill) => {
                                    if let Some(trade_info) = open_pos.apply_close_fill(&fill) {
                                        clean_up = true;
                                        let _ = info_sender
                                            .send(MarketCommand::ReceiveTrade(trade_info))
                                            .await;
                                        info!("Trade Closed: {:?}", trade_info);
                                    }
                                }
                                LimitOrderResponseLocal::Resting(oid) => {
                                    self.resting_orders.insert(oid.oid, limit_order);
                                    println!("{:?}", &self.resting_orders);
                                }
                            }
                        };

                    };
                    if clean_up{
                        drop(maybe_open);
                        let _ = self.cancel_all_resting().await;
                    }
                }

                _ => {}
            }
        }
    }
}
