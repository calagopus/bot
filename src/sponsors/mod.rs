use serde::{Deserialize, Serialize};
use std::sync::{Arc, LazyLock};

mod currency;
pub use currency::*;
mod evaluate;
pub use evaluate::*;
pub mod github;
pub mod opencollective;
mod task;
pub use task::*;

/// Treats a value the API added since this was written as absent, so one unknown enum
/// variant cannot fail the whole page.
pub fn unknown_as_none<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let value = serde_json::Value::deserialize(deserializer)?;

    Ok(T::deserialize(value).ok())
}

#[derive(Deserialize, Serialize, utoipa::ToSchema, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Provider {
    #[serde(rename = "github")]
    GitHub,
    #[serde(rename = "opencollective")]
    OpenCollective,
}

impl Provider {
    pub const ALL: [Self; 2] = [Self::GitHub, Self::OpenCollective];

    #[inline]
    pub fn key(self) -> &'static str {
        match self {
            Self::GitHub => "github",
            Self::OpenCollective => "opencollective",
        }
    }

    #[inline]
    pub fn label(self) -> &'static str {
        match self {
            Self::GitHub => "GitHub Sponsors",
            Self::OpenCollective => "Open Collective",
        }
    }

    #[inline]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|provider| provider.key() == key)
    }

    #[inline]
    pub fn is_configured(self, env: &crate::env::Env) -> bool {
        match self {
            Self::GitHub => github::is_configured(env),
            Self::OpenCollective => opencollective::is_configured(env),
        }
    }

    #[inline]
    pub fn contribute_url(self, env: &crate::env::Env) -> String {
        match self {
            Self::GitHub => format!("https://github.com/sponsors/{}", github::login(env)),
            Self::OpenCollective => format!(
                "https://opencollective.com/{}",
                opencollective::slug(env).unwrap_or_default()
            ),
        }
    }

    #[inline]
    pub fn fallback_avatar_url(self, env: &crate::env::Env) -> String {
        match self {
            Self::GitHub => format!("https://github.com/{}.png", github::login(env)),
            Self::OpenCollective => format!(
                "https://images.opencollective.com/{}/avatar.png",
                opencollective::slug(env).unwrap_or_default()
            ),
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Interval {
    Monthly,
    Yearly,
}

impl Interval {
    #[inline]
    pub fn label(self) -> &'static str {
        match self {
            Self::Monthly => "month",
            Self::Yearly => "year",
        }
    }

    #[inline]
    pub fn months(self) -> i64 {
        match self {
            Self::Monthly => 1,
            Self::Yearly => 12,
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct SponsorAccount {
    pub provider: Provider,
    /// Matches `linked_accounts.provider_id`.
    pub id: String,
    pub login: String,
    pub name: Option<String>,
    pub url: String,
    pub avatar_url: String,
}

/// One one-time payment, or one uninterrupted run of a recurring contribution.
///
/// The `*_in_cents` amounts are normalised to [`Ledger::currency`], the `native_*` ones
/// are kept in the currency the contributor was charged in. Both are what the
/// contributor paid: gross of the platform's own fees, and net of refunds the provider
/// reports per contribution.
#[derive(Debug, Clone)]
pub struct Contribution {
    pub provider: Provider,
    /// Stable across polls, stored as `sent_sponsorships.id`.
    pub id: String,
    /// Groups contributions by contributor, even when the contributor is anonymous.
    pub key: String,
    /// `None` when the contribution is private or incognito.
    pub account: Option<SponsorAccount>,

    pub recurring: bool,
    /// `None` for one-time contributions.
    pub interval: Option<Interval>,
    pub start: chrono::DateTime<chrono::Utc>,
    pub end: Option<chrono::DateTime<chrono::Utc>>,

    /// Headline figure: what is charged per [`Contribution::interval`], or the one-time payment.
    pub amount_in_cents: i64,
    /// The recurring rate, zero for one-time contributions. Only counted towards a
    /// contributor's monthly total while the run is active.
    pub monthly_in_cents: i64,
    pub paid_in_cents: i64,
    pub months_paid: u32,

    pub native_currency: String,
    pub native_amount_in_cents: i64,
    pub native_paid_in_cents: i64,
}

impl Contribution {
    #[inline]
    pub fn active(&self) -> bool {
        self.end.is_none()
    }
}

/// A refund the provider reports against a contributor rather than a single contribution.
#[derive(Debug, Clone)]
pub struct Refund {
    pub key: String,
    pub recurring: bool,
    pub amount_in_cents: i64,
}

#[derive(Debug)]
pub struct ProviderLedger {
    pub provider: Provider,
    pub contributions: Vec<Contribution>,
    pub refunds: Vec<Refund>,
    /// The provider's own monthly figure, used to sanity check the reconstruction.
    pub reported_monthly_in_cents: Option<i64>,
}

#[derive(Debug)]
pub struct Ledger {
    pub currency: String,
    pub providers: Vec<ProviderLedger>,
    pub failed_providers: Vec<Provider>,
}

impl Ledger {
    pub fn is_complete_for(&self, accounts: &[(Provider, String)]) -> bool {
        accounts
            .iter()
            .all(|(provider, _)| !self.failed_providers.contains(provider))
    }
}

pub static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .user_agent(format!("Calagopus Bot ({})", crate::VERSION))
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .expect("failed to build sponsors http client")
});

static LEDGER_CACHE: LazyLock<moka::future::Cache<(), Arc<Ledger>>> = LazyLock::new(|| {
    moka::future::Cache::builder()
        .time_to_live(std::time::Duration::from_secs(300))
        .max_capacity(1)
        .build()
});

#[inline]
pub fn is_configured(env: &crate::env::Env) -> bool {
    Provider::ALL
        .into_iter()
        .any(|provider| provider.is_configured(env))
}

/// Collects every configured provider. A provider that fails is logged and skipped, so one
/// broken integration cannot stall the other; the error only surfaces when none succeed.
pub async fn collect(env: &crate::env::Env) -> Result<Ledger, anyhow::Error> {
    let mut providers = Vec::new();
    let mut failed_providers = Vec::new();
    let mut last_error = None;

    for provider in Provider::ALL {
        if !provider.is_configured(env) {
            continue;
        }

        let collected = match provider {
            Provider::GitHub => github::collect(env).await,
            Provider::OpenCollective => opencollective::collect(env).await,
        };

        match collected {
            Ok(ledger) => providers.push(ledger),
            Err(err) => {
                tracing::error!("failed to collect {} sponsors: {:?}", provider.key(), err);
                failed_providers.push(provider);
                last_error = Some(err);
            }
        }
    }

    if providers.is_empty()
        && let Some(err) = last_error
    {
        return Err(err);
    }

    Ok(Ledger {
        currency: env.sponsors_currency.clone(),
        providers,
        failed_providers,
    })
}

pub async fn refresh_ledger(env: &crate::env::Env) -> Result<Arc<Ledger>, anyhow::Error> {
    let ledger = Arc::new(collect(env).await?);
    LEDGER_CACHE.insert((), ledger.clone()).await;

    Ok(ledger)
}

pub async fn cached_ledger(env: &crate::env::Env) -> Result<Arc<Ledger>, anyhow::Error> {
    LEDGER_CACHE
        .try_get_with((), async { collect(env).await.map(Arc::new) })
        .await
        .map_err(|err| anyhow::anyhow!("{err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_failures_only_block_affected_members() {
        for failed_provider in Provider::ALL {
            let healthy_provider = Provider::ALL
                .into_iter()
                .find(|provider| *provider != failed_provider)
                .unwrap();
            let ledger = Ledger {
                currency: "USD".to_string(),
                providers: vec![ProviderLedger {
                    provider: healthy_provider,
                    contributions: Vec::new(),
                    refunds: Vec::new(),
                    reported_monthly_in_cents: None,
                }],
                failed_providers: vec![failed_provider],
            };

            assert!(ledger.is_complete_for(&[(healthy_provider, "healthy".to_string())]));
            assert!(!ledger.is_complete_for(&[(failed_provider, "failed".to_string())]));
            assert!(!ledger.is_complete_for(&[
                (healthy_provider, "healthy".to_string()),
                (failed_provider, "failed".to_string()),
            ]));
            assert!(ledger.is_complete_for(&[]));
        }
    }

    #[test]
    fn recovery_and_disabled_providers_do_not_block_role_sync() {
        let accounts = vec![(Provider::OpenCollective, "account".to_string())];
        let mut ledger = Ledger {
            currency: "USD".to_string(),
            providers: Vec::new(),
            failed_providers: vec![Provider::OpenCollective],
        };

        assert!(!ledger.is_complete_for(&accounts));
        ledger.failed_providers.clear();
        assert!(ledger.is_complete_for(&accounts));

        ledger.providers.push(ProviderLedger {
            provider: Provider::OpenCollective,
            contributions: Vec::new(),
            refunds: Vec::new(),
            reported_monthly_in_cents: None,
        });
        assert!(ledger.is_complete_for(&accounts));
    }
}
