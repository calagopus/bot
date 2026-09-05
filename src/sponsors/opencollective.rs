use super::{Contribution, Interval, Provider, ProviderLedger, SponsorAccount, unknown_as_none};
use serde::Deserialize;
use std::collections::HashMap;

pub const GRAPHQL_URL: &str = "https://api.opencollective.com/graphql/v2";

const PAGE_SIZE: usize = 100;

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OrderStatus {
    New,
    RequireClientConfirmation,
    Paid,
    Error,
    Processing,
    Rejected,
    Active,
    Cancelled,
    Pending,
    Expired,
    Disputed,
    Refunded,
    Paused,
    InReview,
}

impl OrderStatus {
    /// Whether a recurring contribution has stopped for good. `ERROR`, `DISPUTED` and
    /// `IN_REVIEW` are deliberately left out, they resolve on their own.
    #[inline]
    fn is_ended(self) -> bool {
        matches!(
            self,
            Self::Cancelled | Self::Paused | Self::Expired | Self::Rejected | Self::Refunded
        )
    }
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContributionFrequency {
    Monthly,
    Yearly,
    Onetime,
}

impl ContributionFrequency {
    #[inline]
    fn interval(self) -> Option<Interval> {
        match self {
            Self::Monthly => Some(Interval::Monthly),
            Self::Yearly => Some(Interval::Yearly),
            Self::Onetime => None,
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Amount {
    /// The API returns nullable, floating-point cents.
    #[serde(default)]
    value_in_cents: Option<f64>,
    #[serde(default)]
    currency: Option<String>,
}

impl Amount {
    #[inline]
    fn cents(&self) -> i64 {
        self.value_in_cents.unwrap_or(0.0).round() as i64
    }

    #[inline]
    fn currency<'a>(&'a self, fallback: &'a str) -> &'a str {
        self.currency.as_deref().unwrap_or(fallback)
    }
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct OpenCollectiveAccount {
    pub id: String,
    pub slug: String,
    pub name: Option<String>,
    pub image_url: Option<String>,
    pub is_incognito: bool,
}

impl OpenCollectiveAccount {
    fn account(&self) -> SponsorAccount {
        SponsorAccount {
            provider: Provider::OpenCollective,
            id: self.id.clone(),
            login: self.slug.clone(),
            name: self.name.clone(),
            url: format!("https://opencollective.com/{}", self.slug),
            avatar_url: self.image_url.clone().unwrap_or_else(|| {
                format!("https://images.opencollective.com/{}/avatar.png", self.slug)
            }),
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Order {
    pub id: String,
    #[serde(default, deserialize_with = "unknown_as_none")]
    pub status: Option<OrderStatus>,
    #[serde(default, deserialize_with = "unknown_as_none")]
    pub frequency: Option<ContributionFrequency>,
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
    pub amount: Amount,
    pub total_contributed: Amount,
    pub from_account: Option<OpenCollectiveAccount>,
}

impl Order {
    fn contribution(
        self,
        amount_in_cents: i64,
        paid_in_cents: i64,
        native_currency: &str,
        native_amount_in_cents: i64,
        native_paid_in_cents: i64,
        charges: u32,
    ) -> Option<Contribution> {
        let (status, frequency, created_at) = (self.status?, self.frequency?, self.created_at?);

        if matches!(status, OrderStatus::New | OrderStatus::Pending)
            || (paid_in_cents <= 0 && status != OrderStatus::Refunded)
        {
            return None;
        }

        let interval = frequency.interval();
        let recurring = interval.is_some();
        let end = if recurring {
            status
                .is_ended()
                .then(|| self.updated_at.unwrap_or(created_at))
        } else {
            Some(created_at)
        };

        let account = self.from_account.as_ref();
        let key = account
            .map(|account| account.id.clone())
            .unwrap_or_else(|| format!("order:{}", self.id));

        Some(Contribution {
            provider: Provider::OpenCollective,
            id: self.id,
            key,
            account: account
                .filter(|account| !account.is_incognito)
                .map(|account| account.account()),

            recurring,
            interval,
            start: created_at,
            end,

            amount_in_cents,
            monthly_in_cents: match interval {
                Some(interval) => amount_in_cents / interval.months(),
                None => 0,
            },
            paid_in_cents,
            months_paid: match interval {
                Some(interval) => charges.max(1) * interval.months() as u32,
                None => 1,
            },

            native_currency: native_currency.to_string(),
            native_amount_in_cents,
            native_paid_in_cents,
        })
    }
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct OrderReference {
    pub id: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Transaction {
    pub is_refund: Option<bool>,
    pub is_refunded: Option<bool>,
    /// Gross, so a contribution counts for what the contributor paid rather than what
    /// survived the payment processor's fees, matching what GitHub Sponsors reports.
    pub amount_in_host_currency: Amount,
    pub order: Option<OrderReference>,
}

#[derive(Deserialize)]
pub struct Collection<T> {
    #[serde(rename = "totalCount")]
    pub total_count: usize,
    pub nodes: Vec<T>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecurringBreakdown {
    pub label: Option<String>,
    pub amount: Option<Amount>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountStats {
    pub active_recurring_contributions_breakdown: Vec<RecurringBreakdown>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountMeta {
    pub currency: String,
    pub stats: AccountStats,
}

#[derive(Deserialize)]
struct GraphQlResponse<T> {
    data: Option<T>,
    errors: Option<Vec<serde_json::Value>>,
}

#[derive(Deserialize)]
struct MetaData {
    account: Option<AccountMeta>,
}

#[derive(Deserialize)]
struct OrdersData {
    orders: Collection<Order>,
}

#[derive(Deserialize)]
struct TransactionsData {
    transactions: Collection<Transaction>,
}

#[inline]
pub fn is_configured(env: &crate::env::Env) -> bool {
    env.opencollective_slug.is_some()
}

#[inline]
pub fn slug(env: &crate::env::Env) -> Option<&str> {
    env.opencollective_slug.as_deref()
}

async fn request<T: serde::de::DeserializeOwned>(
    env: &crate::env::Env,
    query: &str,
    variables: serde_json::Value,
) -> Result<T, anyhow::Error> {
    let mut request = super::CLIENT
        .post(GRAPHQL_URL)
        .json(&serde_json::json!({ "query": query, "variables": variables }));

    if let Some(token) = &env.opencollective_token {
        request = request.header("Personal-Token", token);
    }

    let body = request.send().await?.error_for_status()?.text().await?;

    let response: GraphQlResponse<T> = serde_json::from_str(&body).map_err(|err| {
        anyhow::anyhow!("failed to decode open collective response: {err} ({body:.512})")
    })?;

    if let Some(errors) = response.errors.filter(|errors| !errors.is_empty()) {
        return Err(anyhow::anyhow!(
            "open collective graphql errors: {:?}",
            errors
        ));
    }

    response
        .data
        .ok_or_else(|| anyhow::anyhow!("open collective graphql response contained no data"))
}

const META_QUERY: &str = r#"
query($slug: String!) {
    account(slug: $slug) {
        currency
        stats {
            activeRecurringContributionsBreakdown {
                label
                amount {
                    valueInCents
                    currency
                }
            }
        }
    }
}
"#;

const ORDERS_QUERY: &str = r#"
query($slug: String!, $limit: Int!, $offset: Int!) {
    orders(
        account: { slug: $slug }
        filter: INCOMING
        includeIncognito: true
        includeChildrenAccounts: true
        limit: $limit
        offset: $offset
        orderBy: { field: CREATED_AT, direction: ASC }
    ) {
        totalCount
        nodes {
            id
            status
            frequency
            createdAt
            updatedAt
            amount {
                valueInCents
                currency
            }
            totalContributed {
                valueInCents
                currency
            }
            fromAccount {
                id
                slug
                name
                imageUrl
                isIncognito
            }
        }
    }
}
"#;

const TRANSACTIONS_QUERY: &str = r#"
query($slug: String!, $limit: Int!, $offset: Int!) {
    transactions(
        account: { slug: $slug }
        type: CREDIT
        kind: CONTRIBUTION
        hasOrder: true
        isRefund: false
        includeIncognitoTransactions: true
        includeChildrenTransactions: true
        limit: $limit
        offset: $offset
        orderBy: { field: CREATED_AT, direction: ASC }
    ) {
        totalCount
        nodes {
            isRefund
            isRefunded
            amountInHostCurrency {
                valueInCents
                currency
            }
            order {
                id
            }
        }
    }
}
"#;

async fn collect_pages<T, D, F>(
    env: &crate::env::Env,
    slug: &str,
    query: &str,
    pick: F,
) -> Result<Vec<T>, anyhow::Error>
where
    D: serde::de::DeserializeOwned,
    F: Fn(D) -> Collection<T>,
{
    let mut collected = Vec::new();
    let mut offset = 0;

    loop {
        let collection = pick(
            request::<D>(
                env,
                query,
                serde_json::json!({ "slug": slug, "limit": PAGE_SIZE, "offset": offset }),
            )
            .await?,
        );

        let total_count = collection.total_count;
        let received = collection.nodes.len();
        collected.extend(collection.nodes);

        offset += received;
        if received == 0 || offset >= total_count {
            break;
        }
    }

    Ok(collected)
}

/// Settled charges per order in the fiscal host's currency.
struct Settled {
    currency: String,
    paid_in_cents: i64,
    charges: u32,
}

pub async fn collect(env: &crate::env::Env) -> Result<ProviderLedger, anyhow::Error> {
    let Some(slug) = slug(env) else {
        return Err(anyhow::anyhow!("OPENCOLLECTIVE_SLUG is not configured"));
    };

    let meta: MetaData = request(env, META_QUERY, serde_json::json!({ "slug": slug })).await?;
    let Some(meta) = meta.account else {
        return Err(anyhow::anyhow!(
            "open collective account {slug} does not exist"
        ));
    };

    let orders: Vec<Order> =
        collect_pages(env, slug, ORDERS_QUERY, |data: OrdersData| data.orders).await?;
    let transactions: Vec<Transaction> =
        collect_pages(env, slug, TRANSACTIONS_QUERY, |data: TransactionsData| {
            data.transactions
        })
        .await?;

    let currency = env.sponsors_currency.as_str();
    let mut settled: HashMap<String, Settled> = HashMap::new();

    for transaction in transactions {
        if transaction.is_refund.unwrap_or(false) || transaction.is_refunded.unwrap_or(false) {
            continue;
        }

        let Some(order) = transaction.order else {
            continue;
        };

        let amount = &transaction.amount_in_host_currency;
        let host_currency = amount.currency(&meta.currency);

        let entry = settled.entry(order.id).or_insert_with(|| Settled {
            currency: host_currency.to_string(),
            paid_in_cents: 0,
            charges: 0,
        });

        // An order that moved fiscal hosts keeps its charges in the first host's currency.
        entry.paid_in_cents +=
            super::convert(env, amount.cents(), host_currency, &entry.currency).await?;
        entry.charges += 1;
    }

    let mut contributions = Vec::new();

    for order in orders {
        let (Some(status), Some(_), Some(_)) = (order.status, order.frequency, order.created_at)
        else {
            continue;
        };

        if matches!(status, OrderStatus::New | OrderStatus::Pending) {
            continue;
        }

        let settled = settled.remove(&order.id);
        let charges = settled.as_ref().map(|settled| settled.charges).unwrap_or(0);
        let native_currency = order.amount.currency(&meta.currency).to_string();

        // `totalContributed` is the order's own figure and is used only when no settled
        // transaction was found, which happens for contributions recorded off-platform.
        let (paid_currency, paid_in_cents) = match &settled {
            Some(settled) => (settled.currency.as_str(), settled.paid_in_cents),
            None => (
                order.total_contributed.currency(&meta.currency),
                order.total_contributed.cents(),
            ),
        };

        let native_paid_in_cents =
            super::convert(env, paid_in_cents, paid_currency, &native_currency).await?;
        let paid_in_cents = super::convert(env, paid_in_cents, paid_currency, currency).await?;

        if paid_in_cents <= 0 && status != OrderStatus::Refunded {
            continue;
        }

        let native_amount_in_cents = order.amount.cents();
        let amount_in_cents =
            super::convert(env, native_amount_in_cents, &native_currency, currency).await?;

        if let Some(contribution) = order.contribution(
            amount_in_cents,
            paid_in_cents,
            &native_currency,
            native_amount_in_cents,
            native_paid_in_cents,
            charges,
        ) {
            contributions.push(contribution);
        }
    }

    let mut reported_monthly_in_cents = 0;
    for breakdown in meta.stats.active_recurring_contributions_breakdown {
        let Some(amount) = breakdown.amount else {
            continue;
        };

        let months = match breakdown.label.as_deref() {
            Some("yearly") => 12,
            _ => 1,
        };

        reported_monthly_in_cents += super::convert(
            env,
            amount.cents(),
            amount.currency(&meta.currency),
            currency,
        )
        .await?
            / months;
    }

    Ok(ProviderLedger {
        provider: Provider::OpenCollective,
        contributions,
        refunds: Vec::new(),
        reported_monthly_in_cents: Some(reported_monthly_in_cents),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order(status: &str, frequency: &str) -> Order {
        serde_json::from_value(serde_json::json!({
            "id": "order-1",
            "status": status,
            "frequency": frequency,
            "createdAt": "2026-01-01T00:00:00Z",
            "updatedAt": "2026-02-01T00:00:00Z",
            "amount": { "valueInCents": 1200, "currency": "USD" },
            "totalContributed": { "valueInCents": 0, "currency": "USD" },
            "fromAccount": null
        }))
        .unwrap()
    }

    fn contribution(
        order: Order,
        amount_in_cents: i64,
        paid_in_cents: i64,
        charges: u32,
    ) -> Option<Contribution> {
        order.contribution(
            amount_in_cents,
            paid_in_cents,
            "USD",
            amount_in_cents,
            paid_in_cents,
            charges,
        )
    }

    fn evaluate(contribution: Contribution) -> super::super::Evaluation {
        super::super::evaluate(&super::super::Ledger {
            currency: "USD".to_string(),
            providers: vec![ProviderLedger {
                provider: Provider::OpenCollective,
                contributions: vec![contribution],
                refunds: Vec::new(),
                reported_monthly_in_cents: None,
            }],
            failed_providers: Vec::new(),
        })
    }

    #[test]
    fn unpaid_orders_are_not_announced_or_evaluated() {
        for status in [
            "NEW",
            "PENDING",
            "REQUIRE_CLIENT_CONFIRMATION",
            "ERROR",
            "PROCESSING",
            "ACTIVE",
            "PAID",
            "REJECTED",
            "CANCELLED",
            "EXPIRED",
            "DISPUTED",
            "PAUSED",
            "IN_REVIEW",
        ] {
            for frequency in ["MONTHLY", "YEARLY", "ONETIME"] {
                for paid_in_cents in [0, -100] {
                    assert!(
                        contribution(order(status, frequency), 1200, paid_in_cents, 0).is_none(),
                        "{status}, {frequency}, {paid_in_cents}"
                    );
                }
            }
        }
    }

    #[test]
    fn paid_subscriptions_keep_their_roles_during_payment_errors() {
        for status in ["ERROR", "REQUIRE_CLIENT_CONFIRMATION", "PROCESSING"] {
            let evaluation =
                evaluate(contribution(order(status, "MONTHLY"), 1200, 2400, 2).unwrap());

            assert_eq!(evaluation.monthly_recurring_in_cents, 1200);
            assert_eq!(evaluation.lifetime_in_cents, 2400);
            assert_eq!(evaluation.active_sponsor_count, 1);
        }
    }

    #[test]
    fn positive_off_platform_totals_qualify_without_transaction_rows() {
        let evaluation = evaluate(contribution(order("ACTIVE", "YEARLY"), 1200, 1200, 0).unwrap());

        assert_eq!(evaluation.monthly_recurring_in_cents, 100);
        assert_eq!(evaluation.lifetime_in_cents, 1200);
        assert_eq!(evaluation.sponsors[0].estimated_months_paid, 12);
    }

    #[test]
    fn native_amounts_are_kept_next_to_the_ledger_ones() {
        let contribution = order("ACTIVE", "MONTHLY")
            .contribution(1162, 2324, "EUR", 1000, 2000, 2)
            .unwrap();

        assert_eq!(contribution.amount_in_cents, 1162);
        assert_eq!(contribution.paid_in_cents, 2324);
        assert_eq!(contribution.native_currency, "EUR");
        assert_eq!(contribution.native_amount_in_cents, 1000);
        assert_eq!(contribution.native_paid_in_cents, 2000);
    }

    #[test]
    fn refunded_subscriptions_remain_available_for_message_updates() {
        let evaluation = evaluate(contribution(order("REFUNDED", "MONTHLY"), 1200, 0, 0).unwrap());

        assert_eq!(evaluation.contributions.len(), 1);
        assert!(!evaluation.contributions[0].active());
        assert_eq!(evaluation.monthly_recurring_in_cents, 0);
        assert_eq!(evaluation.lifetime_in_cents, 0);
    }
}
