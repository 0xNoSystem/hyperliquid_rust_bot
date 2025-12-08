use std::collections::HashMap;
use std::sync::Arc;

use alloy::signers::local::PrivateKeySigner;
use flume::Receiver;
use log::info;
use tokio::{
    sync::{Mutex, mpsc::Sender},
    time::{Duration, sleep},
};

use rustc_hash::FxHasher;
use std::hash::BuildHasherDefault;

use hyperliquid_rust_sdk::{
    BaseUrl, ClientLimit, ClientOrder, ClientOrderRequest, ClientTrigger, Error, ExchangeClient,
    ExchangeDataStatus, ExchangeResponseStatus, MarketOrderParams, RestingOrder,
};

use crate::market::MarketCommand;
use crate::trade_setup::{
    LimitOrderLocal,LimitOrderResponseLocal, Tif, TradeCommand, TradeFillInfo, TradeInfo, TriggerKind,
};

pub struct Executor {
    trade_rv: Receiver<TradeCommand>,
    market_tx: Sender<MarketCommand>,
    asset: String,
    exchange_client: Arc<ExchangeClient>,
    is_paused: bool,
    fees: (f64, f64),
    resting_orders: HashMap<RestingOrder, LimitOrderLocal, BuildHasherDefault<FxHasher>>,
    open_position: Arc<Mutex<Option<TradeFillInfo>>>,
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
    pub async fn market_open(&self, size: f64, is_long: bool) -> Result<TradeFillInfo, Error> {
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
                let fill_info = TradeFillInfo {
                    fill_type: "Open".to_string(),
                    sz,
                    price,
                    oid: order.oid,
                    is_long,
                };

                Ok(fill_info)
            }

            _ => Err(Error::Custom("Market open order failed".to_string())),
        }
    }
    pub async fn market_close(&self, size: f64, is_long: bool) -> Result<TradeFillInfo, Error> {
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

                let fill_info = TradeFillInfo {
                    fill_type: "Close".to_string(),
                    sz,
                    price,
                    oid: order.oid,
                    is_long,
                };
                Ok(fill_info)
            }

            _ => Err(Error::Custom("Close market order not filled".to_string())),
        }
    }

    pub async fn market_close_static(
        client: Arc<ExchangeClient>,
        asset: String,
        size: f64,
        is_long: bool,
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
                let fill_info = TradeFillInfo {
                    fill_type: "Close".to_string(),
                    sz,
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
        LimitOrderLocal {
            size,
            is_long,
            limit_px,
            tif,
        }: LimitOrderLocal,
    ) -> Result<LimitOrderResponseLocal, Error> {
        let order = ClientOrderRequest {
            asset: self.asset.clone(),
            is_buy: is_long,
            reduce_only: false,
            limit_px,
            sz: size,
            cloid: None,
            order_type: ClientOrder::Limit(ClientLimit {
                tif: tif.to_string(),
            }),
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
                let fill_info = TradeFillInfo {
                    fill_type: "Open".to_string(),
                    sz,
                    price,
                    oid: order.oid,
                    is_long,
                };

                Ok(LimitOrderResponseLocal::Filled(fill_info))
            },

            ExchangeDataStatus::Resting(order) => {
                Ok(LimitOrderResponseLocal::Resting(order))
            },

            ExchangeDataStatus::Error(err) => {
                Err(Error::Custom(err)) 
            },

            _ => Err(Error::Custom("Limit open order failed due to an unexpected exchange status response".to_string())),
        }
    }
     async fn limit_close(
        &self,
        LimitOrderLocal {
            size,
            is_long,
            limit_px,
            tif,
        }: LimitOrderLocal,
    ) -> Result<LimitOrderResponseLocal, Error>{
        
        let order = ClientOrderRequest {
            asset: self.asset.clone(),
            is_buy: !is_long,
            reduce_only: true,
            limit_px,
            sz: size,
            cloid: None,
            order_type: ClientOrder::Limit(ClientLimit {
                tif: tif.to_string(),
            }),
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
                let fill_info = TradeFillInfo {
                    fill_type: "Close".to_string(),
                    sz,
                    price,
                    oid: order.oid,
                    is_long,
                };

                Ok(LimitOrderResponseLocal::Filled(fill_info))
            },

            ExchangeDataStatus::Resting(order) => {
                Ok(LimitOrderResponseLocal::Resting(order))
            },

            ExchangeDataStatus::Error(err) => {
                Err(Error::Custom(err)) 
            },

            _ => Err(Error::Custom("Limit open order failed due to an unexpected exchange status response".to_string())),
        }

    }

     /*
        limit_close_static(client, asset, size, price, is_long)
     */

    fn get_trade_info(open: TradeFillInfo, close: TradeFillInfo, fees: &(f64, f64)) -> TradeInfo {
        let is_long = open.is_long;
        let (fee, pnl) = Self::calculate_pnl(fees, is_long, &open, &close);

        TradeInfo {
            open: open.price,
            close: close.price,
            pnl,
            fee,
            is_long,
            duration: None,
            oid: (open.oid, close.oid),
        }
    }

    fn calculate_pnl(
        fees: &(f64, f64),
        is_long: bool,
        trade_fill_open: &TradeFillInfo,
        trade_fill_close: &TradeFillInfo,
    ) -> (f64, f64) {
        let fee_open = trade_fill_open.sz * trade_fill_open.price * fees.1;
        let fee_close = trade_fill_close.sz * trade_fill_close.price * fees.1;

        let pnl = if is_long {
            trade_fill_close.sz * (trade_fill_close.price - trade_fill_open.price)
                - fee_open
                - fee_close
        } else {
            trade_fill_close.sz * (trade_fill_open.price - trade_fill_close.price)
                - fee_open
                - fee_close
        };

        (fee_open + fee_close, pnl)
    }

    pub async fn cancel_trade(&mut self) -> Option<TradeInfo> {
        if let Some(pos) = self.open_position.lock().await.take() {
            let trade_fill = self.market_close(pos.sz, pos.is_long).await;
            if let Ok(close) = trade_fill {
                let trade_info = Self::get_trade_info(pos, close, &self.fees);
                return Some(trade_info);
            }
        }

        None
    }

    async fn is_active(&self) -> bool {
        let guard = self.open_position.lock().await;
        guard.is_some()
    }

    fn toggle_pause(&mut self) {
        self.is_paused = !self.is_paused
    }

    pub async fn start(&mut self) {
        println!("EXECUTOR STARTED");

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
                    if let Ok(trade_fill) = trade_info {
                        {
                            let mut pos = self.open_position.lock().await;
                            *pos = Some(trade_fill.clone());
                        }

                        let client = self.exchange_client.clone();
                        let asset = self.asset.clone();
                        let fees = self.fees;
                        let sender = info_sender.clone();
                        let pos_handle = self.open_position.clone();
                        tokio::spawn(async move {
                            let _ = sleep(Duration::from_secs(duration)).await;
                            let maybe_open = {
                                let mut pos = pos_handle.lock().await;
                                pos.take()
                            };

                            if let Some(open) = maybe_open {
                                let close_fill =
                                    Self::market_close_static(client, asset, open.sz, is_long)
                                        .await;
                                if let Ok(fill) = close_fill {
                                    let trade_info = Self::get_trade_info(open, fill, &fees);

                                    let _ =
                                        sender.send(MarketCommand::ReceiveTrade(trade_info)).await;
                                    info!("Trade Closed: {:?}", trade_info);
                                }
                            }
                        });
                    };
                }

                TradeCommand::OpenTrade { size, is_long } => {
                    info!("Open trade command received");

                    if !self.is_active().await && !self.is_paused {
                        let trade_fill = self.market_open(size, is_long).await;

                        if let Ok(trade) = trade_fill {
                            info!("Trade Opened: {:?}", trade.clone());
                            *self.open_position.lock().await = Some(trade);
                        };
                    } else if self.is_active().await {
                        info!("OpenTrade skipped: a trade is already active");
                    }
                }

                TradeCommand::CloseTrade { size } => {
                    if self.is_paused {
                        continue;
                    };
                    let maybe_open = {
                        let mut pos = self.open_position.lock().await;
                        pos.take()
                    };

                    if let Some(open_pos) = maybe_open {
                        let size = size.min(open_pos.sz);
                        let trade_fill = self.market_close(size, open_pos.is_long).await;

                        if let Ok(fill) = trade_fill {
                            let trade_info = Self::get_trade_info(open_pos, fill, &self.fees);
                            let _ = info_sender
                                .send(MarketCommand::ReceiveTrade(trade_info))
                                .await;
                            info!("Trade Closed: {:?}", trade_info);
                        };
                    };
                }

                TradeCommand::CancelTrade => {
                    if let Some(trade_info) = self.cancel_trade().await {
                        let _ = info_sender
                            .send(MarketCommand::ReceiveTrade(trade_info))
                            .await;
                    };

                    return;
                }

                TradeCommand::Liquidation(liq_fill) => {
                    let maybe_open = {
                        let mut pos = self.open_position.lock().await;
                        pos.take()
                    };

                    if let Some(open_pos) = maybe_open {
                        let liq_fill: TradeFillInfo = liq_fill.into();
                        println!(
                            "MAKE SURE SIZES ARE THE SAME: \nLocal {open_pos:?}\nLiquidation: {liq_fill:?}"
                        );
                        let trade_info = Self::get_trade_info(open_pos, liq_fill, &self.fees);

                        let _ = info_sender
                            .send(MarketCommand::ReceiveTrade(trade_info))
                            .await;
                        info!("LIQUIDATION INFO: {:?}", trade_info);
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
                    };
                    self.is_paused = true;
                }
                TradeCommand::Resume => {
                    self.is_paused = false;
                }

                //TradeCommand::BuildPosition{size, is_long, interval} => {info!("Contacting Bob the builder")},
                _ => {}
            }
        }
    }
}
