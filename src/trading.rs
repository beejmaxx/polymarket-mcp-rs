use std::{
    collections::HashMap,
    fmt,
    path::{Path, PathBuf},
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
            Amount, AssetType, OrderType, Side, SignatureType, SignedOrder, TraderSide,
            request::{
                BalanceAllowanceRequest, CancelMarketOrderRequest, OrdersRequest, TradesRequest,
            },
            response::{CancelOrdersResponse, OpenOrderResponse, PostOrderResponse, TradeResponse},
        },
    },
    types::{Address, B256, Decimal, U256},
};
use rusqlite::{Connection, OptionalExtension as _, params};
use tokio::sync::Mutex;

use crate::{
    error::AppError,
    polymarket::CLOB_V2_ENDPOINT,
    types::{
        AccountTrade, AccountTradesOutput, BalanceAllowanceOutput, BatchPlacedOrdersOutput,
        CancelFailure, CancelOrdersOutput, ContractAllowance, OpenOrder, OpenOrdersOutput,
        OrderApprovalStatusOutput, OrderPreviewOutput, PlacedOrderOutput, TradingStatusOutput,
        UserRealtimeEventsOutput, UserRealtimeStatusOutput, UserWatchInfo,
    },
    user_realtime::UserRealtimeService,
};

type AuthClient = ClobClient<Authenticated<Normal>>;

#[derive(Clone)]
pub struct TradingService {
    enabled: bool,
    signer: Option<PrivateKeySigner>,
    signature_type: SignatureType,
    funder: Option<Address>,
    max_notional: Decimal,
    approvals: Arc<Mutex<HashMap<String, OrderPlan>>>,
    context: Arc<Mutex<Option<AuthContext>>>,
    user_realtime: UserRealtimeService,
    next_id: Arc<AtomicU64>,
    audit_path: PathBuf,
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

pub(crate) struct PreviewParameters {
    pub token_id: U256,
    pub kind: String,
    pub side: String,
    pub amount: Decimal,
    pub price: Option<Decimal>,
    pub order_type: Option<String>,
    pub market_rules: Option<(Decimal, Decimal)>,
}

impl fmt::Debug for TradingService {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TradingService")
            .field("enabled", &self.enabled)
            .field("signer_configured", &self.signer.is_some())
            .field("signature_type", &self.signature_type)
            .field("funder_configured", &self.funder.is_some())
            .field("max_notional", &self.max_notional)
            .finish_non_exhaustive()
    }
}

impl TradingService {
    /// Construct a credential-blind trading service for public HTTP deployments.
    /// Environment keys and enablement flags are intentionally ignored.
    pub fn disabled(audit_path: PathBuf) -> Self {
        Self {
            enabled: false,
            signer: None,
            signature_type: SignatureType::Eoa,
            funder: None,
            max_notional: Decimal::from(100),
            approvals: Arc::new(Mutex::new(HashMap::new())),
            context: Arc::new(Mutex::new(None)),
            user_realtime: UserRealtimeService::new(),
            next_id: Arc::new(AtomicU64::new(1)),
            audit_path,
        }
    }

    pub fn from_environment(audit_path: PathBuf) -> Result<Self, AppError> {
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
        let signature_type = parse_signature_type(
            &std::env::var("POLYMARKET_SIGNATURE_TYPE").unwrap_or_else(|_| "eoa".to_owned()),
        )?;
        let funder = std::env::var("POLYMARKET_FUNDER_ADDRESS")
            .ok()
            .map(|value| {
                Address::from_str(value.trim()).map_err(|error| {
                    AppError::InvalidInput(format!("invalid POLYMARKET_FUNDER_ADDRESS: {error}"))
                })
            })
            .transpose()?;
        match (signature_type, funder) {
            (SignatureType::Eoa, Some(_)) => {
                return Err(AppError::InvalidInput(
                    "POLYMARKET_FUNDER_ADDRESS must be omitted for EOA signature type".to_owned(),
                ));
            }
            (SignatureType::Poly1271, None) => {
                return Err(AppError::InvalidInput(
                    "POLYMARKET_FUNDER_ADDRESS is required for poly1271 signature type".to_owned(),
                ));
            }
            _ => {}
        }
        let max_notional = std::env::var("POLYMARKET_MAX_ORDER_PUSD")
            .or_else(|_| std::env::var("POLYMARKET_MAX_ORDER_USDC"))
            .unwrap_or_else(|_| "100".to_owned())
            .parse::<Decimal>()
            .map_err(|error| {
                AppError::InvalidInput(format!("invalid POLYMARKET_MAX_ORDER_PUSD: {error}"))
            })?;
        if max_notional <= Decimal::ZERO {
            return Err(AppError::InvalidInput(
                "POLYMARKET_MAX_ORDER_PUSD must be greater than zero".to_owned(),
            ));
        }
        Ok(Self {
            enabled,
            signer,
            signature_type,
            funder,
            max_notional,
            approvals: Arc::new(Mutex::new(HashMap::new())),
            context: Arc::new(Mutex::new(None)),
            user_realtime: UserRealtimeService::new(),
            next_id: Arc::new(AtomicU64::new(1)),
            audit_path,
        })
    }

    #[must_use]
    pub fn status(&self) -> TradingStatusOutput {
        TradingStatusOutput {
            enabled: self.enabled,
            signer_configured: self.signer.is_some(),
            signer_address: self.signer.as_ref().map(|signer| signer.address().to_string()),
            signature_type: self.signature_type.to_string(),
            funder_address: self.funder.map(|address| address.to_string()),
            cancel_on_disconnect_enabled: false,
            max_order_notional_pusd: self.max_notional.to_string(),
            safety_model: "Trading is off unless POLYMARKET_ENABLE_TRADING=true. Placement requires a single-use five-minute preview approval and confirm=true; every order is capped by POLYMARKET_MAX_ORDER_PUSD. Account reads do not arm cancel-on-disconnect. Cancellation requires separate explicit confirmation.".to_owned(),
        }
    }

    pub(crate) async fn preview(
        &self,
        parameters: PreviewParameters,
    ) -> Result<OrderPreviewOutput, AppError> {
        let PreviewParameters {
            token_id,
            kind,
            side,
            amount,
            price,
            order_type,
            market_rules,
        } = parameters;
        if amount <= Decimal::ZERO {
            return Err(AppError::InvalidInput(
                "amount must be greater than zero".to_owned(),
            ));
        }
        let side = parse_side(&side)?;
        let kind = kind.trim().to_ascii_lowercase();
        let (order_type, price, amount_unit, notional, mut warnings) = match kind.as_str() {
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
                let unit = if side == Side::Buy { "pusd" } else { "shares" };
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
                "maximum notional {notional} exceeds configured limit {} pUSD",
                self.max_notional
            )));
        }
        if let Some((tick_size, min_order_size)) = market_rules {
            if kind == "limit" && price.is_some_and(|price| price % tick_size != Decimal::ZERO) {
                return Err(AppError::InvalidInput(format!(
                    "limit price must be a multiple of the current tick size {tick_size}"
                )));
            }
            if (kind == "limit" || side == Side::Sell) && amount < min_order_size {
                return Err(AppError::InvalidInput(format!(
                    "share amount {amount} is below the current minimum order size {min_order_size}"
                )));
            }
            warnings.push(format!(
                "Validated against current tick size {tick_size} and minimum order size {min_order_size}."
            ));
        } else {
            warnings.push(
                "Live exchange-rule validation was explicitly disabled; acceptance is less certain."
                    .to_owned(),
            );
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
        insert_approval_audit(&self.audit_path, &approval_id, &plan)?;
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
            maximum_notional_pusd: notional.to_string(),
            live_validation_performed: market_rules.is_some(),
            current_tick_size: market_rules.map(|(tick, _)| tick.to_string()),
            current_min_order_size: market_rules.map(|(_, minimum)| minimum.to_string()),
            warnings,
        })
    }

    pub async fn approval_status(
        &self,
        approval_id: &str,
    ) -> Result<OrderApprovalStatusOutput, AppError> {
        let path = self.audit_path.clone();
        let approval_id = approval_id.to_owned();
        tokio::task::spawn_blocking(move || read_approval_audit(&path, &approval_id))
            .await
            .map_err(trading_audit_error)?
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
            .get(approval_id)
            .cloned()
            .ok_or_else(|| {
                AppError::InvalidInput(format!("unknown or already-used approval_id {approval_id}"))
            })?;
        if now_ms() > plan.expires_at_ms {
            set_approval_audit(
                &self.audit_path,
                approval_id,
                "expired",
                &[],
                Some("approval expired before submission"),
            )?;
            return Err(AppError::InvalidInput(
                "order approval has expired".to_owned(),
            ));
        }
        transition_approval_audit(&self.audit_path, approval_id, "approved", "validating")?;
        let mut context = self.context.lock().await;
        if let Err(error) = self.ensure_authenticated(&mut context).await {
            set_approval_audit(
                &self.audit_path,
                approval_id,
                "approved",
                &[],
                Some(&error.to_string()),
            )?;
            return Err(error);
        }
        let context = context.as_ref().unwrap();
        if let Err(error) = ensure_not_geoblocked(&context.client).await {
            set_approval_audit(
                &self.audit_path,
                approval_id,
                "approved",
                &[],
                Some(&error.to_string()),
            )?;
            return Err(error);
        }
        let signable_result = if plan.kind == "limit" {
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
            };
            let amount = match amount {
                Ok(amount) => amount,
                Err(error) => {
                    let error = trading_error(error);
                    set_approval_audit(
                        &self.audit_path,
                        approval_id,
                        "approved",
                        &[],
                        Some(&error.to_string()),
                    )?;
                    return Err(error);
                }
            };
            context
                .client
                .market_order()
                .token_id(plan.token_id)
                .side(plan.side)
                .amount(amount)
                .order_type(plan.order_type)
                .build()
                .await
        };
        let signable = match signable_result {
            Ok(order) => order,
            Err(error) => {
                let error = trading_error(error);
                set_approval_audit(
                    &self.audit_path,
                    approval_id,
                    "approved",
                    &[],
                    Some(&error.to_string()),
                )?;
                return Err(error);
            }
        };
        let signed = match context.client.sign(&context.signer, signable).await {
            Ok(order) => order,
            Err(error) => {
                let error = trading_error(error);
                set_approval_audit(
                    &self.audit_path,
                    approval_id,
                    "approved",
                    &[],
                    Some(&error.to_string()),
                )?;
                return Err(error);
            }
        };
        set_approval_audit(&self.audit_path, approval_id, "submitting", &[], None)?;
        let response = match context.client.post_order(signed).await {
            Ok(response) => response,
            Err(error) => {
                let error = trading_error(error);
                self.approvals.lock().await.remove(approval_id);
                set_approval_audit(
                    &self.audit_path,
                    approval_id,
                    "unknown",
                    &[],
                    Some(&error.to_string()),
                )?;
                return Err(error);
            }
        };
        let response = placed_order(response);
        self.approvals.lock().await.remove(approval_id);
        set_approval_audit(
            &self.audit_path,
            approval_id,
            "submitted",
            std::slice::from_ref(&response.order_id),
            response.error_message.as_deref(),
        )?;
        Ok(response)
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
        let approvals = self.approvals.lock().await;
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
                set_approval_audit(
                    &self.audit_path,
                    approval_id,
                    "expired",
                    &[],
                    Some("approval expired before batch submission"),
                )?;
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
                "batch maximum notional {batch_notional} exceeds configured limit {} pUSD",
                self.max_notional
            )));
        }
        drop(approvals);
        transition_approval_batch(&self.audit_path, &approval_ids, "approved", "validating")?;

        let mut context = self.context.lock().await;
        if let Err(error) = self.ensure_authenticated(&mut context).await {
            set_approval_batch(
                &self.audit_path,
                &approval_ids,
                "approved",
                Some(&error.to_string()),
            )?;
            return Err(error);
        }
        let context = context.as_ref().unwrap();
        if let Err(error) = ensure_not_geoblocked(&context.client).await {
            set_approval_batch(
                &self.audit_path,
                &approval_ids,
                "approved",
                Some(&error.to_string()),
            )?;
            return Err(error);
        }
        let signed_orders = match build_signed_batch(context, &plans).await {
            Ok(orders) => orders,
            Err(error) => {
                set_approval_batch(
                    &self.audit_path,
                    &approval_ids,
                    "approved",
                    Some(&error.to_string()),
                )?;
                return Err(error);
            }
        };
        set_approval_batch(&self.audit_path, &approval_ids, "submitting", None)?;
        let responses = match context.client.post_orders(signed_orders).await {
            Ok(responses) => responses,
            Err(error) => {
                let error = trading_error(error);
                for approval_id in &approval_ids {
                    self.approvals.lock().await.remove(approval_id);
                }
                set_approval_batch(
                    &self.audit_path,
                    &approval_ids,
                    "unknown",
                    Some(&error.to_string()),
                )?;
                return Err(error);
            }
        };
        let orders = responses.into_iter().map(placed_order).collect::<Vec<_>>();
        for (index, approval_id) in approval_ids.iter().enumerate() {
            self.approvals.lock().await.remove(approval_id);
            if let Some(order) = orders.get(index) {
                set_approval_audit(
                    &self.audit_path,
                    approval_id,
                    "submitted",
                    std::slice::from_ref(&order.order_id),
                    order.error_message.as_deref(),
                )?;
            } else {
                set_approval_audit(
                    &self.audit_path,
                    approval_id,
                    "unknown",
                    &[],
                    Some("batch response omitted the corresponding order result"),
                )?;
            }
        }
        Ok(BatchPlacedOrdersOutput {
            count: orders.len(),
            orders,
        })
    }

    pub async fn order(&self, order_id: &str) -> Result<OpenOrder, AppError> {
        self.require_signer()?;
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

    pub async fn open_orders(
        &self,
        token_id: Option<U256>,
        next_cursor: Option<String>,
    ) -> Result<OpenOrdersOutput, AppError> {
        self.require_signer()?;
        let mut context = self.context.lock().await;
        self.ensure_authenticated(&mut context).await?;
        let request = OrdersRequest::builder().maybe_asset_id(token_id).build();
        let page = context
            .as_ref()
            .unwrap()
            .client
            .orders(&request, next_cursor)
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
        next_cursor: Option<String>,
    ) -> Result<AccountTradesOutput, AppError> {
        self.require_signer()?;
        let mut context = self.context.lock().await;
        self.ensure_authenticated(&mut context).await?;
        let request = TradesRequest::builder().maybe_asset_id(token_id).build();
        let page = context
            .as_ref()
            .unwrap()
            .client
            .trades(&request, next_cursor)
            .await
            .map_err(trading_error)?;
        Ok(AccountTradesOutput {
            count: page.data.len(),
            next_cursor: (!page.next_cursor.is_empty()).then_some(page.next_cursor),
            trades: page.data.into_iter().map(account_trade).collect(),
        })
    }

    pub async fn watch_user_events(
        &self,
        condition_ids: Vec<B256>,
    ) -> Result<UserWatchInfo, AppError> {
        self.require_signer()?;
        let (credentials, address) = {
            let mut context = self.context.lock().await;
            self.ensure_authenticated(&mut context).await?;
            let client = &context.as_ref().unwrap().client;
            (client.credentials().clone(), client.address())
        };
        self.user_realtime
            .watch(credentials, address, condition_ids)
            .await
    }

    pub async fn user_events(
        &self,
        watch_id: &str,
        after_sequence: u64,
        limit: usize,
    ) -> Result<UserRealtimeEventsOutput, AppError> {
        self.user_realtime
            .events(watch_id, after_sequence, limit)
            .await
    }

    pub async fn user_realtime_status(&self) -> UserRealtimeStatusOutput {
        self.user_realtime.status().await
    }

    pub async fn stop_user_watch(&self, watch_id: &str) -> crate::types::StopWatchOutput {
        self.user_realtime.stop(watch_id).await
    }

    pub async fn stop_user_watches(&self) -> usize {
        self.user_realtime.stop_all().await
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

    pub async fn balance_allowance(
        &self,
        asset_type: AssetType,
        token_id: Option<U256>,
    ) -> Result<BalanceAllowanceOutput, AppError> {
        self.require_signer()?;
        if asset_type == AssetType::Conditional && token_id.is_none() {
            return Err(AppError::InvalidInput(
                "token_id is required when asset_type is conditional".to_owned(),
            ));
        }
        if asset_type == AssetType::Collateral && token_id.is_some() {
            return Err(AppError::InvalidInput(
                "token_id must be omitted when asset_type is collateral".to_owned(),
            ));
        }
        let mut context = self.context.lock().await;
        self.ensure_authenticated(&mut context).await?;
        let request = BalanceAllowanceRequest::builder()
            .asset_type(asset_type.clone())
            .maybe_token_id(token_id)
            .build();
        let response = context
            .as_ref()
            .unwrap()
            .client
            .balance_allowance(request)
            .await
            .map_err(trading_error)?;
        let mut allowances = response
            .allowances
            .into_iter()
            .map(|(contract, allowance)| ContractAllowance {
                contract: contract.to_string(),
                allowance,
            })
            .collect::<Vec<_>>();
        allowances.sort_by(|left, right| left.contract.cmp(&right.contract));
        Ok(BalanceAllowanceOutput {
            asset_type: asset_type.to_string(),
            token_id: token_id.map(|value| value.to_string()),
            balance: response.balance.to_string(),
            allowances,
        })
    }

    pub async fn cancel_market(
        &self,
        condition_id: Option<B256>,
        token_id: Option<U256>,
        confirmation: &str,
    ) -> Result<CancelOrdersOutput, AppError> {
        self.require_enabled()?;
        if confirmation != "CANCEL_MARKET" {
            return Err(AppError::InvalidInput(
                "confirmation must exactly equal CANCEL_MARKET".to_owned(),
            ));
        }
        if condition_id.is_none() && token_id.is_none() {
            return Err(AppError::InvalidInput(
                "provide condition_id, token_id, or both".to_owned(),
            ));
        }
        let mut context = self.context.lock().await;
        self.ensure_authenticated(&mut context).await?;
        let request = CancelMarketOrderRequest::builder()
            .maybe_market(condition_id)
            .maybe_asset_id(token_id)
            .build();
        let response = context
            .as_ref()
            .unwrap()
            .client
            .cancel_market_orders(&request)
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
        self.require_signer()
    }

    fn require_signer(&self) -> Result<(), AppError> {
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
        .signature_type(self.signature_type);
        let client = if let Some(funder) = self.funder {
            client.funder(funder)
        } else {
            client
        }
        .authenticate()
        .await
        .map_err(trading_error)?;
        *context = Some(AuthContext { client, signer });
        Ok(())
    }
}

fn parse_signature_type(value: &str) -> Result<SignatureType, AppError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "0" | "eoa" => Ok(SignatureType::Eoa),
        "1" | "proxy" | "poly_proxy" => Ok(SignatureType::Proxy),
        "2" | "safe" | "gnosis" | "gnosis_safe" => Ok(SignatureType::GnosisSafe),
        "3" | "poly1271" | "poly_1271" => Ok(SignatureType::Poly1271),
        _ => Err(AppError::InvalidInput(
            "POLYMARKET_SIGNATURE_TYPE must be eoa, proxy, gnosis_safe, or poly1271".to_owned(),
        )),
    }
}

async fn build_signed_batch(
    context: &AuthContext,
    plans: &[(String, OrderPlan)],
) -> Result<Vec<SignedOrder>, AppError> {
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
                .order_type(plan.order_type.clone())
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
                .order_type(plan.order_type.clone())
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
    Ok(signed_orders)
}

async fn ensure_not_geoblocked(client: &AuthClient) -> Result<(), AppError> {
    let status = client.check_geoblock().await.map_err(trading_error)?;
    if status.blocked {
        return Err(AppError::InvalidInput(format!(
            "trading is unavailable from the detected region {}-{}",
            status.country, status.region
        )));
    }
    Ok(())
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

fn initialize_trading_audit(connection: &Connection) -> Result<(), AppError> {
    connection
        .execute_batch(
            "PRAGMA journal_mode = WAL;
             CREATE TABLE IF NOT EXISTS order_approval_audit (
                approval_id TEXT PRIMARY KEY,
                status TEXT NOT NULL,
                created_at_ms INTEGER NOT NULL,
                updated_at_ms INTEGER NOT NULL,
                expires_at_ms INTEGER NOT NULL,
                token_id TEXT NOT NULL,
                kind TEXT NOT NULL,
                side TEXT NOT NULL,
                amount TEXT NOT NULL,
                price TEXT,
                order_type TEXT NOT NULL,
                order_ids_json TEXT NOT NULL DEFAULT '[]',
                last_error TEXT
             );",
        )
        .map_err(trading_audit_error)
}

fn insert_approval_audit(path: &Path, approval_id: &str, plan: &OrderPlan) -> Result<(), AppError> {
    let connection = Connection::open(path).map_err(trading_audit_error)?;
    initialize_trading_audit(&connection)?;
    let created_at_ms = now_ms();
    connection
        .execute(
            "INSERT INTO order_approval_audit (approval_id, status, created_at_ms, updated_at_ms, expires_at_ms, token_id, kind, side, amount, price, order_type) VALUES (?1, 'approved', ?2, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                approval_id,
                as_i64(created_at_ms),
                as_i64(plan.expires_at_ms),
                plan.token_id.to_string(),
                plan.kind,
                plan.side.to_string(),
                plan.amount.to_string(),
                plan.price.map(|value| value.to_string()),
                plan.order_type.to_string(),
            ],
        )
        .map_err(trading_audit_error)?;
    Ok(())
}

fn transition_approval_audit(
    path: &Path,
    approval_id: &str,
    expected: &str,
    next: &str,
) -> Result<(), AppError> {
    let connection = Connection::open(path).map_err(trading_audit_error)?;
    initialize_trading_audit(&connection)?;
    let changed = connection
        .execute(
            "UPDATE order_approval_audit SET status = ?3, updated_at_ms = ?4, last_error = NULL WHERE approval_id = ?1 AND status = ?2",
            params![approval_id, expected, next, as_i64(now_ms())],
        )
        .map_err(trading_audit_error)?;
    if changed != 1 {
        return Err(AppError::InvalidInput(format!(
            "approval_id {approval_id} is not in the expected {expected} state"
        )));
    }
    Ok(())
}

fn transition_approval_batch(
    path: &Path,
    approval_ids: &[String],
    expected: &str,
    next: &str,
) -> Result<(), AppError> {
    let mut connection = Connection::open(path).map_err(trading_audit_error)?;
    initialize_trading_audit(&connection)?;
    let transaction = connection.transaction().map_err(trading_audit_error)?;
    for approval_id in approval_ids {
        let changed = transaction
            .execute(
                "UPDATE order_approval_audit SET status = ?3, updated_at_ms = ?4, last_error = NULL WHERE approval_id = ?1 AND status = ?2",
                params![approval_id, expected, next, as_i64(now_ms())],
            )
            .map_err(trading_audit_error)?;
        if changed != 1 {
            return Err(AppError::InvalidInput(format!(
                "approval_id {approval_id} is not in the expected {expected} state"
            )));
        }
    }
    transaction.commit().map_err(trading_audit_error)
}

fn set_approval_audit(
    path: &Path,
    approval_id: &str,
    status: &str,
    order_ids: &[String],
    last_error: Option<&str>,
) -> Result<(), AppError> {
    let connection = Connection::open(path).map_err(trading_audit_error)?;
    initialize_trading_audit(&connection)?;
    let order_ids = serde_json::to_string(order_ids).map_err(trading_audit_error)?;
    connection
        .execute(
            "UPDATE order_approval_audit SET status = ?2, updated_at_ms = ?3, order_ids_json = ?4, last_error = ?5 WHERE approval_id = ?1",
            params![approval_id, status, as_i64(now_ms()), order_ids, last_error],
        )
        .map_err(trading_audit_error)?;
    Ok(())
}

fn set_approval_batch(
    path: &Path,
    approval_ids: &[String],
    status: &str,
    last_error: Option<&str>,
) -> Result<(), AppError> {
    let mut connection = Connection::open(path).map_err(trading_audit_error)?;
    initialize_trading_audit(&connection)?;
    let transaction = connection.transaction().map_err(trading_audit_error)?;
    for approval_id in approval_ids {
        transaction
            .execute(
                "UPDATE order_approval_audit SET status = ?2, updated_at_ms = ?3, last_error = ?4 WHERE approval_id = ?1",
                params![approval_id, status, as_i64(now_ms()), last_error],
            )
            .map_err(trading_audit_error)?;
    }
    transaction.commit().map_err(trading_audit_error)
}

fn read_approval_audit(
    path: &Path,
    approval_id: &str,
) -> Result<OrderApprovalStatusOutput, AppError> {
    let connection = Connection::open(path).map_err(trading_audit_error)?;
    initialize_trading_audit(&connection)?;
    connection
        .query_row(
            "SELECT approval_id, status, created_at_ms, updated_at_ms, expires_at_ms, token_id, kind, side, amount, price, order_type, order_ids_json, last_error FROM order_approval_audit WHERE approval_id = ?1",
            [approval_id],
            |row| {
                let order_ids: String = row.get(11)?;
                let order_ids = serde_json::from_str(&order_ids).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        11,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?;
                Ok(OrderApprovalStatusOutput {
                    approval_id: row.get(0)?,
                    status: row.get(1)?,
                    created_at_ms: row.get::<_, i64>(2)?.max(0) as u64,
                    updated_at_ms: row.get::<_, i64>(3)?.max(0) as u64,
                    expires_at_ms: row.get::<_, i64>(4)?.max(0) as u64,
                    token_id: row.get(5)?,
                    kind: row.get(6)?,
                    side: row.get(7)?,
                    amount: row.get(8)?,
                    price: row.get(9)?,
                    order_type: row.get(10)?,
                    order_ids,
                    last_error: row.get(12)?,
                })
            },
        )
        .optional()
        .map_err(trading_audit_error)?
        .ok_or_else(|| AppError::InvalidInput(format!("unknown approval_id {approval_id}")))
}

fn trading_audit_error(error: impl ToString) -> AppError {
    AppError::Upstream {
        service: "SQLite trading audit",
        message: error.to_string(),
    }
}

fn as_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
