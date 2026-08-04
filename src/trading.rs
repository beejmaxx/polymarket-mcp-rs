use std::{
    collections::HashMap,
    fmt,
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use alloy::signers::{Signer as _, local::PrivateKeySigner};
use polymarket_client_sdk_v2::{
    POLYGON,
    auth::{Normal, state::Authenticated},
    clob::{
        Client as ClobClient, Config as ClobConfig,
        types::{
            Amount, OrderType, Side, TraderSide,
            request::{OrdersRequest, TradesRequest},
            response::{CancelOrdersResponse, OpenOrderResponse, PostOrderResponse, TradeResponse},
        },
    },
    types::{Decimal, U256},
};
use tokio::sync::Mutex;

use crate::{
    error::AppError,
    polymarket::CLOB_V2_ENDPOINT,
    types::{
        AccountTrade, AccountTradesOutput, BatchPlacedOrdersOutput, CancelFailure,
        CancelOrdersOutput, OpenOrder, OpenOrdersOutput, OrderPreviewOutput, PlacedOrderOutput,
        TradingStatusOutput,
    },
};

type AuthClient = ClobClient<Authenticated<Normal>>;

#[derive(Clone)]
pub struct TradingService {
    enabled: bool,
    signer: Option<PrivateKeySigner>,
    max_notional: Decimal,
    approvals: Arc<Mutex<HashMap<String, OrderPlan>>>,
    context: Arc<Mutex<Option<AuthContext>>>,
    next_id: Arc<AtomicU64>,
}

struct AuthContext {
    client: AuthClient,
    signer: PrivateKeySigner,
}

#[derive(Clone, Debug)]
struct OrderPlan {
    token_id: U256,
    kind: String,
    side: Side,
    amount: Decimal,
    price: Option<Decimal>,
    order_type: OrderType,
    expires_at_ms: u64,
}

impl fmt::Debug for TradingService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TradingService")
            .field("enabled", &self.enabled)
            .field("signer_configured", &self.signer.is_some())
            .field("max_notional", &self.max_notional)
            .finish_non_exhaustive()
    }
}

impl TradingService {
    pub fn from_environment() -> Result<Self, AppError> {
        let enabled = std::env::var("POLYMARKET_ENABLE_TRADING")
            .is_ok_and(|value| value.eq_ignore_ascii_case("true"));
        let signer = std::env::var("POLYMARKET_PRIVATE_KEY")
            .ok()
            .map(|value| {
                PrivateKeySigner::from_str(&value)
                    .map(|signer| signer.with_chain_id(Some(POLYGON)))
                    .map_err(|error| {
                        AppError::InvalidInput(format!("invalid POLYMARKET_PRIVATE_KEY: {error}"))
                    })
            })
            .transpose()?;
        let max_notional = std::env::var("POLYMARKET_MAX_ORDER_USDC")
            .unwrap_or_else(|_| "100".to_owned())
            .parse::<Decimal>()
            .map_err(|error| {
                AppError::InvalidInput(format!("invalid POLYMARKET_MAX_ORDER_USDC: {error}"))
            })?;
        if max_notional <= Decimal::ZERO {
            return Err(AppError::InvalidInput(
                "POLYMARKET_MAX_ORDER_USDC must be greater than zero".to_owned(),
            ));
        }
        Ok(Self {
            enabled,
            signer,
            max_notional,
            approvals: Arc::new(Mutex::new(HashMap::new())),
            context: Arc::new(Mutex::new(None)),
            next_id: Arc::new(AtomicU64::new(1)),
        })
    }

    #[must_use]
    pub fn status(&self) -> TradingStatusOutput {
        TradingStatusOutput {
            enabled: self.enabled,
            signer_configured: self.signer.is_some(),
            signer_address: self.signer.as_ref().map(|signer| signer.address().to_string()),
            max_order_notional_usdc: self.max_notional.to_string(),
            safety_model: "Trading is off unless POLYMARKET_ENABLE_TRADING=true. Placement requires a single-use five-minute preview approval and confirm=true; every order is capped by POLYMARKET_MAX_ORDER_USDC. Cancellation requires separate explicit confirmation.".to_owned(),
        }
    }

    pub async fn preview(
        &self,
        token_id: U256,
        kind: String,
        side: String,
        amount: Decimal,
        price: Option<Decimal>,
        order_type: Option<String>,
    ) -> Result<OrderPreviewOutput, AppError> {
        if amount <= Decimal::ZERO {
            return Err(AppError::InvalidInput(
                "amount must be greater than zero".to_owned(),
            ));
        }
        let side = parse_side(&side)?;
        let kind = kind.trim().to_ascii_lowercase();
        let (order_type, price, amount_unit, notional, warnings) = match kind.as_str() {
            "limit" => {
                let price = price.ok_or_else(|| {
                    AppError::InvalidInput("price is required for limit orders".to_owned())
                })?;
                if price <= Decimal::ZERO || price >= Decimal::ONE {
                    return Err(AppError::InvalidInput(
                        "limit price must be greater than 0 and less than 1".to_owned(),
                    ));
                }
                let order_type = order_type.unwrap_or_else(|| "GTC".to_owned());
                if !order_type.eq_ignore_ascii_case("GTC") {
                    return Err(AppError::InvalidInput(
                        "limit orders currently support GTC only".to_owned(),
                    ));
                }
                (
                    OrderType::GTC,
                    Some(price),
                    "shares",
                    price * amount,
                    vec![
                        "A GTC order can remain open until filled or explicitly cancelled."
                            .to_owned(),
                    ],
                )
            }
            "market" => {
                if price.is_some() {
                    return Err(AppError::InvalidInput(
                        "price must be omitted for market orders".to_owned(),
                    ));
                }
                let order_type = match order_type
                    .unwrap_or_else(|| "FOK".to_owned())
                    .to_ascii_uppercase()
                    .as_str()
                {
                    "FOK" => OrderType::FOK,
                    "FAK" => OrderType::FAK,
                    _ => {
                        return Err(AppError::InvalidInput(
                            "market order_type must be FOK or FAK".to_owned(),
                        ));
                    }
                };
                let unit = if side == Side::Buy { "usdc" } else { "shares" };
                (order_type, None, unit, amount, vec!["Market execution depends on book changes, latency, balance, allowances, and fees; preview is a policy check, not a guaranteed fill.".to_owned()])
            }
            _ => {
                return Err(AppError::InvalidInput(
                    "kind must be limit or market".to_owned(),
                ));
            }
        };
        if notional > self.max_notional {
            return Err(AppError::InvalidInput(format!(
                "maximum notional {notional} exceeds configured limit {} USDC",
                self.max_notional
            )));
        }
        let expires_at_ms = now_ms().saturating_add(5 * 60 * 1_000);
        let approval_id = format!(
            "approval-{}-{}",
            now_ms(),
            self.next_id.fetch_add(1, Ordering::Relaxed)
        );
        let plan = OrderPlan {
            token_id,
            kind: kind.clone(),
            side,
            amount,
            price,
            order_type: order_type.clone(),
            expires_at_ms,
        };
        self.approvals
            .lock()
            .await
            .insert(approval_id.clone(), plan);
        Ok(OrderPreviewOutput {
            approval_id,
            expires_at_ms,
            token_id: token_id.to_string(),
            kind,
            side: side.to_string(),
            amount: amount.to_string(),
            amount_unit: amount_unit.to_owned(),
            price: price.map(|value| value.to_string()),
            order_type: order_type.to_string(),
            maximum_notional_usdc: notional.to_string(),
            warnings,
        })
    }

    pub async fn place(
        &self,
        approval_id: &str,
        confirm: bool,
    ) -> Result<PlacedOrderOutput, AppError> {
        self.require_enabled()?;
        if !confirm {
            return Err(AppError::InvalidInput("confirm must be true".to_owned()));
        }
        let plan = self
            .approvals
            .lock()
            .await
            .remove(approval_id)
            .ok_or_else(|| {
                AppError::InvalidInput(format!("unknown or already-used approval_id {approval_id}"))
            })?;
        if now_ms() > plan.expires_at_ms {
            return Err(AppError::InvalidInput(
                "order approval has expired".to_owned(),
            ));
        }
        let mut context = self.context.lock().await;
        self.ensure_authenticated(&mut context).await?;
        let context = context.as_ref().unwrap();
        let response = if plan.kind == "limit" {
            context
                .client
                .limit_order()
                .token_id(plan.token_id)
                .side(plan.side)
                .price(plan.price.unwrap())
                .size(plan.amount)
                .order_type(plan.order_type)
                .build_sign_and_post(&context.signer)
                .await
        } else {
            let amount = if plan.side == Side::Buy {
                Amount::usdc(plan.amount)
            } else {
                Amount::shares(plan.amount)
            }
            .map_err(trading_error)?;
            context
                .client
                .market_order()
                .token_id(plan.token_id)
                .side(plan.side)
                .amount(amount)
                .order_type(plan.order_type)
                .build_sign_and_post(&context.signer)
                .await
        }
        .map_err(trading_error)?;
        Ok(placed_order(response))
    }

    pub async fn place_batch(
        &self,
        approval_ids: Vec<String>,
        confirmation: &str,
    ) -> Result<BatchPlacedOrdersOutput, AppError> {
        self.require_enabled()?;
        if confirmation != "PLACE_BATCH" {
            return Err(AppError::InvalidInput(
                "confirmation must exactly equal PLACE_BATCH".to_owned(),
            ));
        }
        if !(1..=10).contains(&approval_ids.len()) {
            return Err(AppError::InvalidInput(
                "approval_ids must contain between 1 and 10 IDs".to_owned(),
            ));
        }
        let mut approvals = self.approvals.lock().await;
        let mut plans = Vec::with_capacity(approval_ids.len());
        for approval_id in &approval_ids {
            if plans
                .iter()
                .any(|(id, _): &(String, OrderPlan)| id == approval_id)
            {
                return Err(AppError::InvalidInput(format!(
                    "duplicate approval_id {approval_id}"
                )));
            }
            let plan = approvals.get(approval_id).cloned().ok_or_else(|| {
                AppError::InvalidInput(format!("unknown or already-used approval_id {approval_id}"))
            })?;
            if now_ms() > plan.expires_at_ms {
                return Err(AppError::InvalidInput(format!(
                    "order approval {approval_id} has expired"
                )));
            }
            plans.push((approval_id.clone(), plan));
        }
        let batch_notional = plans.iter().fold(Decimal::ZERO, |sum, (_, plan)| {
            sum + plan.price.map_or(plan.amount, |price| price * plan.amount)
        });
        if batch_notional > self.max_notional {
            return Err(AppError::InvalidInput(format!(
                "batch maximum notional {batch_notional} exceeds configured limit {} USDC",
                self.max_notional
            )));
        }
        for (approval_id, _) in &plans {
            approvals.remove(approval_id);
        }
        drop(approvals);

        let mut context = self.context.lock().await;
        self.ensure_authenticated(&mut context).await?;
        let context = context.as_ref().unwrap();
        let mut signed_orders = Vec::with_capacity(plans.len());
        for (_, plan) in plans {
            let signable = if plan.kind == "limit" {
                context
                    .client
                    .limit_order()
                    .token_id(plan.token_id)
                    .side(plan.side)
                    .price(plan.price.unwrap())
                    .size(plan.amount)
                    .order_type(plan.order_type)
                    .build()
                    .await
            } else {
                let amount = if plan.side == Side::Buy {
                    Amount::usdc(plan.amount)
                } else {
                    Amount::shares(plan.amount)
                }
                .map_err(trading_error)?;
                context
                    .client
                    .market_order()
                    .token_id(plan.token_id)
                    .side(plan.side)
                    .amount(amount)
                    .order_type(plan.order_type)
                    .build()
                    .await
            }
            .map_err(trading_error)?;
            signed_orders.push(
                context
                    .client
                    .sign(&context.signer, signable)
                    .await
                    .map_err(trading_error)?,
            );
        }
        let orders = context
            .client
            .post_orders(signed_orders)
            .await
            .map_err(trading_error)?
            .into_iter()
            .map(placed_order)
            .collect::<Vec<_>>();
        Ok(BatchPlacedOrdersOutput {
            count: orders.len(),
            orders,
        })
    }

    pub async fn order(&self, order_id: &str) -> Result<OpenOrder, AppError> {
        self.require_enabled()?;
        let mut context = self.context.lock().await;
        self.ensure_authenticated(&mut context).await?;
        let response = context
            .as_ref()
            .unwrap()
            .client
            .order(order_id)
            .await
            .map_err(trading_error)?;
        Ok(open_order(response))
    }

    pub async fn open_orders(&self, token_id: Option<U256>) -> Result<OpenOrdersOutput, AppError> {
        self.require_enabled()?;
        let mut context = self.context.lock().await;
        self.ensure_authenticated(&mut context).await?;
        let request = OrdersRequest::builder().maybe_asset_id(token_id).build();
        let page = context
            .as_ref()
            .unwrap()
            .client
            .orders(&request, None)
            .await
            .map_err(trading_error)?;
        Ok(OpenOrdersOutput {
            count: page.data.len(),
            next_cursor: (!page.next_cursor.is_empty()).then_some(page.next_cursor),
            orders: page.data.into_iter().map(open_order).collect(),
        })
    }

    pub async fn account_trades(
        &self,
        token_id: Option<U256>,
    ) -> Result<AccountTradesOutput, AppError> {
        self.require_enabled()?;
        let mut context = self.context.lock().await;
        self.ensure_authenticated(&mut context).await?;
        let request = TradesRequest::builder().maybe_asset_id(token_id).build();
        let page = context
            .as_ref()
            .unwrap()
            .client
            .trades(&request, None)
            .await
            .map_err(trading_error)?;
        Ok(AccountTradesOutput {
            count: page.data.len(),
            next_cursor: (!page.next_cursor.is_empty()).then_some(page.next_cursor),
            trades: page.data.into_iter().map(account_trade).collect(),
        })
    }

    pub async fn cancel_order(
        &self,
        order_id: &str,
        confirm: bool,
    ) -> Result<CancelOrdersOutput, AppError> {
        self.require_enabled()?;
        if !confirm {
            return Err(AppError::InvalidInput("confirm must be true".to_owned()));
        }
        let mut context = self.context.lock().await;
        self.ensure_authenticated(&mut context).await?;
        let response = context
            .as_ref()
            .unwrap()
            .client
            .cancel_order(order_id)
            .await
            .map_err(trading_error)?;
        Ok(cancel_output(response))
    }

    pub async fn cancel_all(&self, confirmation: &str) -> Result<CancelOrdersOutput, AppError> {
        self.require_enabled()?;
        if confirmation != "CANCEL_ALL" {
            return Err(AppError::InvalidInput(
                "confirmation must exactly equal CANCEL_ALL".to_owned(),
            ));
        }
        let mut context = self.context.lock().await;
        self.ensure_authenticated(&mut context).await?;
        let response = context
            .as_ref()
            .unwrap()
            .client
            .cancel_all_orders()
            .await
            .map_err(trading_error)?;
        Ok(cancel_output(response))
    }

    fn require_enabled(&self) -> Result<(), AppError> {
        if !self.enabled {
            return Err(AppError::InvalidInput(
                "trading is disabled; set POLYMARKET_ENABLE_TRADING=true explicitly".to_owned(),
            ));
        }
        if self.signer.is_none() {
            return Err(AppError::InvalidInput(
                "POLYMARKET_PRIVATE_KEY is not configured".to_owned(),
            ));
        }
        Ok(())
    }

    async fn ensure_authenticated(
        &self,
        context: &mut Option<AuthContext>,
    ) -> Result<(), AppError> {
        if context.is_some() {
            return Ok(());
        }
        let signer = self.signer.clone().ok_or_else(|| {
            AppError::InvalidInput("POLYMARKET_PRIVATE_KEY is not configured".to_owned())
        })?;
        let client = ClobClient::new(
            CLOB_V2_ENDPOINT,
            ClobConfig::builder().use_server_time(true).build(),
        )
        .map_err(trading_error)?
        .authentication_builder(&signer)
        .authenticate()
        .await
        .map_err(trading_error)?;
        *context = Some(AuthContext { client, signer });
        Ok(())
    }
}

fn parse_side(value: &str) -> Result<Side, AppError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "buy" => Ok(Side::Buy),
        "sell" => Ok(Side::Sell),
        _ => Err(AppError::InvalidInput(
            "side must be buy or sell".to_owned(),
        )),
    }
}

fn placed_order(response: PostOrderResponse) -> PlacedOrderOutput {
    PlacedOrderOutput {
        order_id: response.order_id,
        status: response.status.to_string(),
        success: response.success,
        error_message: response.error_msg,
        making_amount: response.making_amount.to_string(),
        taking_amount: response.taking_amount.to_string(),
        transaction_hashes: response
            .transaction_hashes
            .into_iter()
            .map(|value| value.to_string())
            .collect(),
        trade_ids: response.trade_ids,
    }
}

fn open_order(response: OpenOrderResponse) -> OpenOrder {
    OpenOrder {
        order_id: response.id,
        status: response.status.to_string(),
        condition_id: response.market.to_string(),
        token_id: response.asset_id.to_string(),
        side: response.side.to_string(),
        original_size: response.original_size.to_string(),
        size_matched: response.size_matched.to_string(),
        price: response.price.to_string(),
        outcome: response.outcome,
        created_at: response.created_at.to_rfc3339(),
        expiration: response.expiration.to_rfc3339(),
        order_type: response.order_type.to_string(),
    }
}

fn account_trade(response: TradeResponse) -> AccountTrade {
    AccountTrade {
        trade_id: response.id,
        taker_order_id: response.taker_order_id,
        condition_id: response.market.to_string(),
        token_id: response.asset_id.to_string(),
        side: response.side.to_string(),
        size: response.size.to_string(),
        price: response.price.to_string(),
        fee_rate_bps: response.fee_rate_bps.to_string(),
        status: response.status.to_string(),
        match_time: response.match_time.to_rfc3339(),
        last_update: response.last_update.to_rfc3339(),
        outcome: response.outcome,
        transaction_hash: response.transaction_hash.to_string(),
        trader_side: match response.trader_side {
            TraderSide::Taker => "TAKER".to_owned(),
            TraderSide::Maker => "MAKER".to_owned(),
            TraderSide::Unknown(value) => value,
            _ => "UNKNOWN".to_owned(),
        },
        error_message: response.error_msg,
    }
}

fn cancel_output(response: CancelOrdersResponse) -> CancelOrdersOutput {
    let mut not_canceled = response
        .not_canceled
        .into_iter()
        .map(|(order_id, reason)| CancelFailure { order_id, reason })
        .collect::<Vec<_>>();
    not_canceled.sort_by(|left, right| left.order_id.cmp(&right.order_id));
    CancelOrdersOutput {
        canceled: response.canceled,
        not_canceled,
    }
}

fn trading_error(error: impl ToString) -> AppError {
    AppError::Upstream {
        service: "CLOB V2 trading API",
        message: error.to_string(),
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
