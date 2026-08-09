use serde::{Deserialize, Serialize};
use std::sync::{Arc, LazyLock};

mod evaluate;
pub use evaluate::*;
mod task;
pub use task::*;

fn unknown_as_none<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let value = serde_json::Value::deserialize(deserializer)?;

    Ok(T::deserialize(value).ok())
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SponsorsActivityAction {
    NewSponsorship,
    CancelledSponsorship,
    TierChange,
    Refund,
    PendingChange,
    SponsorMatchDisabled,
}

#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SponsorshipPrivacy {
    Public,
    Private,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GitHubSponsor {
    pub database_id: Option<i64>,
    pub login: String,
    pub name: Option<String>,
    pub avatar_url: String,
    pub url: String,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GitHubSponsorsTier {
    pub monthly_price_in_cents: i64,
    pub is_one_time: bool,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GithubSponsorshipNode {
    pub id: String,
    #[serde(default, deserialize_with = "unknown_as_none")]
    pub action: Option<SponsorsActivityAction>,
    pub timestamp: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default, deserialize_with = "unknown_as_none")]
    pub current_privacy_level: Option<SponsorshipPrivacy>,
    pub sponsor: Option<GitHubSponsor>,
    pub sponsors_tier: Option<GitHubSponsorsTier>,
    pub previous_sponsors_tier: Option<GitHubSponsorsTier>,
}

impl GithubSponsorshipNode {
    #[inline]
    pub fn is_public(&self) -> bool {
        self.current_privacy_level == Some(SponsorshipPrivacy::Public)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubSponsorsPageInfo {
    pub end_cursor: Option<String>,
    pub has_next_page: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubSponsorsActivities {
    pub nodes: Vec<Option<GithubSponsorshipNode>>,
    pub page_info: GitHubSponsorsPageInfo,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubSponsorsOrganization {
    pub monthly_estimated_sponsors_income_in_cents: Option<i64>,
    pub sponsors_activities: GitHubSponsorsActivities,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubSponsorsData {
    pub organization: GitHubSponsorsOrganization,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubSponsorsResponse {
    pub data: Option<GitHubSponsorsData>,
    pub errors: Option<Vec<serde_json::Value>>,
}

#[derive(Debug)]
pub struct Ledger {
    pub activities: Vec<GithubSponsorshipNode>,
    pub monthly_estimated_income_in_cents: Option<i64>,
}

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .user_agent(format!("Calagopus Bot ({})", crate::VERSION))
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
pub fn sponsors_login(env: &crate::env::Env) -> &str {
    env.github_sponsors_login.as_deref().unwrap_or("calagopus")
}

fn graphql_query(env: &crate::env::Env, after: Option<&str>) -> String {
    let after = after
        .map(|c| format!(r#", after: "{c}""#))
        .unwrap_or_default();

    let login = sponsors_login(env);

    format!(
        r#"
        query {{
            organization(login: "{login}") {{
                monthlyEstimatedSponsorsIncomeInCents
                sponsorsActivities(first: 100, period: ALL, includePrivate: true, orderBy: {{field: TIMESTAMP, direction: ASC}}{after}) {{
                    nodes {{
                        id
                        action
                        timestamp
                        currentPrivacyLevel
                        sponsorsTier {{
                            monthlyPriceInCents
                            isOneTime
                        }}
                        previousSponsorsTier {{
                            monthlyPriceInCents
                            isOneTime
                        }}
                        sponsor {{
                            ... on User {{
                                databaseId
                                login
                                avatarUrl
                                name
                                url
                            }}
                            ... on Organization {{
                                databaseId
                                login
                                avatarUrl
                                name
                                url
                            }}
                        }}
                    }}
                    pageInfo {{
                        endCursor
                        hasNextPage
                    }}
                }}
            }}
        }}
        "#
    )
}

/// Old to new
pub async fn collect_sponsors(env: &crate::env::Env) -> Result<Ledger, anyhow::Error> {
    let Some(github_token) = &env.github_token else {
        return Err(anyhow::anyhow!("GITHUB_TOKEN is not configured"));
    };

    let mut activities = Vec::new();
    let mut monthly_estimated_income_in_cents = None;
    let mut after = None;

    loop {
        let res = CLIENT
            .post("https://api.github.com/graphql")
            .bearer_auth(github_token)
            .json(&serde_json::json!({ "query": graphql_query(env, after.as_deref()) }))
            .send()
            .await?
            .error_for_status()?;

        let response: GitHubSponsorsResponse = res.json().await?;

        if let Some(errors) = response.errors.filter(|e| !e.is_empty()) {
            return Err(anyhow::anyhow!("github graphql errors: {:?}", errors));
        }

        let Some(data) = response.data else {
            return Err(anyhow::anyhow!("github graphql response contained no data"));
        };

        let organization = data.organization;
        monthly_estimated_income_in_cents = organization
            .monthly_estimated_sponsors_income_in_cents
            .or(monthly_estimated_income_in_cents);

        activities.extend(organization.sponsors_activities.nodes.into_iter().flatten());

        let page_info = organization.sponsors_activities.page_info;
        if !page_info.has_next_page {
            break;
        }

        let Some(end_cursor) = page_info.end_cursor else {
            break;
        };

        after = Some(end_cursor);
    }

    Ok(Ledger {
        activities,
        monthly_estimated_income_in_cents,
    })
}

pub async fn refresh_ledger(env: &crate::env::Env) -> Result<Arc<Ledger>, anyhow::Error> {
    let ledger = Arc::new(collect_sponsors(env).await?);
    LEDGER_CACHE.insert((), ledger.clone()).await;

    Ok(ledger)
}

pub async fn cached_ledger(env: &crate::env::Env) -> Result<Arc<Ledger>, anyhow::Error> {
    LEDGER_CACHE
        .try_get_with((), async { collect_sponsors(env).await.map(Arc::new) })
        .await
        .map_err(|err| anyhow::anyhow!("{err}"))
}
