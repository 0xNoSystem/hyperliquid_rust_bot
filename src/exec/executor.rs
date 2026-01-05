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
    AssetMeta, BaseUrl, ClientCancelRequest, ClientOrderRequest, Error, ExchangeClient,
    ExchangeDataStatus, MarketOrderParams,
};

use super::*;
use crate::{MAX_DECIMALS, MarketCommand, roundf};

pub struct Executor {
    trade_rv: Receiver<ExecCommand>,
    market_tx: Sender<MarketCommand>,
    asset: AssetMeta,
    exchange_client: Arc<ExchangeClient>,
    is_paused: bool,
    resting_orders: HashMap<u64, RestingOrderLocal, BuildHasherDefault<FxHasher>>,
    open_position: Arc<Mutex<Option<OpenPositionLocal>>>,
    decimals: Decimals,
}

impl Executor {
    pub async fn new(
        wallet: PrivateKeySigner,
        asset: AssetMeta,
        trade_rv: Receiver<ExecCommand>,
        market_tx: Sender<MarketCommand>,
    ) -> Result<Executor, Error> {
        let exchange_client =
            Arc::new(ExchangeClient::new(None, wallet, Some(BaseUrl::Mainnet), None, None).await?);

        let px_dec_fix = if asset.name == "SOL" { 2 } else { 1 };
        let decimals = Decimals {
            sz: asset.sz_decimals,
            px: MAX_DECIMALS - asset.sz_decimals - px_dec_fix,
        };
        Ok(Executor {
            trade_rv,
            market_tx,
            asset,
            exchange_client,
            is_paused: false,
            resting_orders: HashMap::default(),
            open_position: Arc::new(Mutex::new(None)),
            decimals,
        })
    }

    async fn with_position<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut Option<OpenPositionLocal>) -> R,
    {
        let mut guard = self.open_position.lock().await;
        let r = f(&mut guard);
        self.update_market(SendUpdate::Position(*guard)).await;
        r
    }

    async fn open_trade(
        &mut self,
        order: HlOrder<'_>,
        intent: PositionOp,
        trigger: Option<TriggerKind>,
    ) -> Result<RestingOrderLocal, Error> {
        let side = order.get_side();
        let limit_px = dbg!(order.get_px());
        let size = order.get_sz();

        let status_res = match order {
            HlOrder::Market(market_order) => self.exchange_client.market_open(market_order).await?,
            HlOrder::Limit(limit_order) => self.exchange_client.order(limit_order, None).await?,
        };

        match extract_order_status(status_res)? {
            ExchangeDataStatus::Filled(fill) => Ok(RestingOrderLocal {
                oid: fill.oid,
                limit_px,
                sz: size,
                side,
                intent,
                tpsl: trigger,
            }),
            ExchangeDataStatus::Resting(res) => Ok(RestingOrderLocal {
                oid: res.oid,
                limit_px,
                sz: size,
                side,
                intent,
                tpsl: trigger,
            }),

            ExchangeDataStatus::Error(err) => Err(Error::Custom(err)),

            _ => Err(Error::ExecutionFailure(
                "unexpected exchange status response".to_string(),
            )),
        }
    }
    async fn cancel_all_resting(&mut self) -> Result<(), Error> {
        let asset = self.asset.name.clone();
        let mut failed_cancels: HashSet<u64> = HashSet::new();
        for (oid, _) in self.resting_orders.drain() {
            let cancel = ClientCancelRequest {
                asset: asset.clone(),
                oid,
            };
            if let Err(e) = self.exchange_client.cancel(cancel, None).await {
                warn!("Failed to cancel oid {}: {:?}", oid, e);
                failed_cancels.insert(oid);
            }
        }
        let mut retries = 0;

        while !failed_cancels.is_empty() {
            retries += 1;
            let iterator = failed_cancels.iter().copied().collect::<Vec<_>>();
            for oid in iterator.iter() {
                let cancel = ClientCancelRequest {
                    asset: asset.clone(),
                    oid: *oid,
                };
                if self.exchange_client.cancel(cancel, None).await.is_ok() {
                    failed_cancels.remove(oid);
                }
            }

            if retries > 5 {
                return Err(Error::Custom(format!(
                    "Failed to cancle resting order for {} market, please cancel manually on https://app.hyperliquid.xyz/trade/{}",
                    &asset, &asset,
                )));
            }
            sleep(Duration::from_millis(100)).await;
        }
        Ok(())
    }

    fn into_hl_order(
        asset: &str,
        sz: f64,
        side: Side,
        limit: Option<Limit>,
        intent: PositionOp,
        decimals: Decimals,
    ) -> HlOrder<'_> {
        let is_long = side == Side::Long;
        let sz = roundf!(sz, decimals.sz);

        if let Some(limit) = limit {
            let reduce_only = (intent == PositionOp::Close) || limit.is_tpsl().is_some();
            let px = roundf!(limit.limit_px, decimals.px);
            HlOrder::Limit(ClientOrderRequest {
                asset: asset.to_string(),
                is_buy: is_long,
                reduce_only,
                limit_px: px,
                sz,
                cloid: None,
                order_type: limit.order_type.convert(px),
            })
        } else {
            HlOrder::Market(MarketOrderParams {
                asset,
                is_buy: is_long,
                sz,
                px: None,
                slippage: None,
                cloid: None,
                wallet: None,
            })
        }
    }
    async fn apply_fill(&mut self, fill: TradeFillInfo) -> Option<TradeInfo> {
        let mut clean_up = false;

        if let Some(resting) = self.resting_orders.get_mut(&fill.oid) {
            assert_eq!(resting.intent, fill.intent);
            if let Some(px) = resting.limit_px
                && resting.tpsl.is_none()
            {
                match resting.side {
                    Side::Long => assert!(fill.price <= px),
                    Side::Short => assert!(fill.price >= px),
                }
            }
            resting.sz -= fill.sz;
            if roundf!(resting.sz, self.asset.sz_decimals) == 0.0 {
                clean_up = true;
            }
        } else if fill.intent != PositionOp::Close {
            info!("Manual trade opened by the user, will be tracked");
        }

        if clean_up {
            self.resting_orders.remove(&fill.oid);
        }

        let trade_info = self
            .with_position(|pos| match fill.intent {
                PositionOp::OpenLong | PositionOp::OpenShort => {
                    if let Some(open_pos) = pos {
                        open_pos.apply_open_fill(&fill);
                    } else {
                        *pos = Some(OpenPositionLocal::new(fill));
                    }
                    None
                }

                PositionOp::Close => {
                    if let Some(open_pos) = pos {
                        let trade = open_pos.apply_close_fill(&fill, self.asset.sz_decimals);
                        if trade.is_some() {
                            *pos = None;
                        }
                        trade
                    } else {
                        None
                    }
                }
            })
            .await;

        //Clean up resting orders in case of user closing a position manually on HL's interface
        if trade_info.is_some() && !clean_up {
            info!(
                "Trade has been closed manually on the exchange, canceling local resting orders..."
            );
            let _ = self.cancel_all_resting().await;
        }

        trade_info
    }

    #[inline]
    async fn update_market(&self, update: SendUpdate) {
        use SendUpdate::*;
        let cmd = match update {
            Trade(trade) => MarketCommand::ReceiveTrade(trade),
            Position(pos) => MarketCommand::UpdateOpenPosition(pos),
        };
        let _ = self.market_tx.send(cmd).await;
    }

    async fn kill(&mut self) {
        let _ = self.cancel_all_resting().await;

        let params = self
            .with_position(|pos| {
                if let Some(open_pos) = pos {
                    Some((!open_pos.side, open_pos.size))
                } else {
                    None
                }
            })
            .await;
        if let Some((side, size)) = params {
            let asset = self.asset.name.clone();
            let op = PositionOp::Close;
            let trade = Self::into_hl_order(&asset, size, side, None, op, self.decimals);
            match self.open_trade(trade, op, None).await {
                Ok(order_response) => {
                    let _ = self
                        .resting_orders
                        .insert(order_response.oid, order_response);
                }
                Err(e) => warn!("{}", e),
            }
        }
    }

    pub async fn start(&mut self) {
        use ExecCommand::*;
        while let Ok(cmd) = self.trade_rv.recv_async().await {
            match cmd {
                Order(order) => {
                    if self.is_paused {
                        continue;
                    }
                    dbg!(&order);
                    let order_params: Option<(Side, f64)> = match order.action {
                        PositionOp::OpenLong => Some((Side::Long, order.size)),
                        PositionOp::OpenShort => Some((Side::Short, order.size)),
                        PositionOp::Close => {
                            self.with_position(|pos| {
                                if let Some(open_pos) = pos {
                                    let size = order.size.min(open_pos.size);
                                    let side = !open_pos.side;
                                    Some((side, size))
                                } else {
                                    None
                                }
                            })
                            .await
                        }
                    };

                    if let Some((side, size)) = order_params {
                        let asset = self.asset.name.clone();
                        let trade = Self::into_hl_order(
                            &asset,
                            size,
                            side,
                            order.limit,
                            order.action,
                            self.decimals,
                        );
                        let trigger = order.is_tpsl();
                        match self.open_trade(trade, order.action, trigger).await {
                            Ok(order_response) => {
                                self.resting_orders
                                    .insert(order_response.oid, order_response);
                            }
                            Err(e) => warn!("{}", e),
                        }
                    }
                }
                Control(control) => match control {
                    ExecControl::Kill => {
                        self.kill().await;
                        return;
                    }
                    ExecControl::Pause => {
                        self.kill().await;
                        self.is_paused = true;
                    }
                    ExecControl::Resume => {
                        self.is_paused = false;
                    }
                    ExecControl::ForceClose => {
                        self.kill().await;
                    }
                },

                Event(event) => {
                    match event {
                        ExecEvent::Fill(fill) => match fill.intent {
                            PositionOp::OpenLong | PositionOp::OpenShort => {
                                let _ = self.apply_fill(fill).await;
                            }
                            PositionOp::Close => {
                                if let Some(trade_info) = self.apply_fill(fill).await {
                                    self.update_market(SendUpdate::Trade(trade_info)).await;
                                }
                            }
                        },
                        ExecEvent::Funding(funding) => {
                            self.with_position(|pos|{
                                if let Some(open_pos) = pos{
                                    open_pos.funding += funding;
                                }else{
                                    warn!("Received position funding but there was no OpenPositionLocal");
                                }}).await;
                        }
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
enum SendUpdate {
    Trade(TradeInfo),
    Position(Option<OpenPositionLocal>),
}

#[derive(Debug, Clone, Copy)]
struct Decimals {
    sz: u32,
    px: u32,
}
