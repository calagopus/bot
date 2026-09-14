use super::{Contribution, Provider, ProviderLedger, Refund, SponsorAccount, unknown_as_none};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// GitHub Sponsors settles in USD, every amount below is in USD cents until the
/// contributions are normalised at the end of [`collect`].
const CURRENCY: &str = "USD";

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

impl GitHubSponsor {
    fn key(&self) -> String {
        match self.database_id {
            Some(id) => id.to_string(),
            None => format!("login:{}", self.login),
        }
    }

    fn account(&self) -> SponsorAccount {
        SponsorAccount {
            provider: Provider::GitHub,
            id: self.key(),
            login: self.login.clone(),
            name: self.name.clone(),
            url: self.url.clone(),
            avatar_url: self.avatar_url.clone(),
        }
    }
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

#[inline]
pub fn is_configured(env: &crate::env::Env) -> bool {
    env.github_token.is_some()
}

#[inline]
pub fn login(env: &crate::env::Env) -> &str {
    env.github_sponsors_login.as_deref().unwrap_or("calagopus")
}

fn graphql_query(env: &crate::env::Env, after: Option<&str>) -> String {
    let after = after
        .map(|c| format!(r#", after: "{c}""#))
        .unwrap_or_default();

    let login = login(env);

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
pub async fn collect_activities(
    env: &crate::env::Env,
) -> Result<(Vec<GithubSponsorshipNode>, Option<i64>), anyhow::Error> {
    let Some(github_token) = &env.github_token else {
        return Err(anyhow::anyhow!("GITHUB_TOKEN is not configured"));
    };

    let mut activities = Vec::new();
    let mut monthly_estimated_income_in_cents = None;
    let mut after = None;

    loop {
        let res = super::CLIENT
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

    Ok((activities, monthly_estimated_income_in_cents))
}

/// An uninterrupted run of a recurring sponsorship, with one segment per tier it moved through.
#[derive(Debug, Clone)]
struct Spell {
    activity_id: String,
    start: chrono::DateTime<chrono::Utc>,
    end: Option<chrono::DateTime<chrono::Utc>>,
    segments: Vec<(chrono::DateTime<chrono::Utc>, i64)>,
}

impl Spell {
    #[inline]
    fn monthly_in_cents(&self) -> i64 {
        self.segments.last().map(|(_, cents)| *cents).unwrap_or(0)
    }

    fn evaluate(&self, now: chrono::DateTime<chrono::Utc>) -> (i64, u32) {
        let end = self.end.unwrap_or(now);

        let mut total = 0;
        let mut months = 0;

        for elapsed in 0.. {
            let Some(charged) = self.start.checked_add_months(chrono::Months::new(elapsed)) else {
                break;
            };

            if charged > end {
                break;
            }

            total += self
                .segments
                .iter()
                .rev()
                .find(|(from, _)| *from <= charged)
                .map(|(_, cents)| *cents)
                .unwrap_or(0);
            months += 1;
        }

        (total, months)
    }
}

#[derive(Default)]
struct SponsorState {
    sponsor: Option<GitHubSponsor>,
    public: bool,
    one_time: Vec<(String, chrono::DateTime<chrono::Utc>, i64)>,
    spells: Vec<Spell>,
    one_time_refunded_in_cents: i64,
    recurring_refunded_in_cents: i64,
}

/// Replays the activity log into contributions, old to new.
fn build(
    activities: &[GithubSponsorshipNode],
    now: chrono::DateTime<chrono::Utc>,
) -> (Vec<Contribution>, Vec<Refund>) {
    let mut states: IndexMap<String, SponsorState> = IndexMap::new();

    for activity in activities {
        let (Some(action), Some(sponsor), Some(timestamp)) = (
            activity.action,
            activity.sponsor.as_ref(),
            activity.timestamp,
        ) else {
            continue;
        };

        let state = states.entry(sponsor.key()).or_default();
        state.sponsor = Some(sponsor.clone());
        state.public = activity.is_public();

        match action {
            SponsorsActivityAction::NewSponsorship => {
                let Some(tier) = activity.sponsors_tier.as_ref() else {
                    continue;
                };

                if tier.is_one_time {
                    state.one_time.push((
                        activity.id.clone(),
                        timestamp,
                        tier.monthly_price_in_cents,
                    ));
                } else {
                    if let Some(open) = state.spells.iter_mut().find(|s| s.end.is_none()) {
                        open.end = Some(timestamp);
                    }

                    state.spells.push(Spell {
                        activity_id: activity.id.clone(),
                        start: timestamp,
                        end: None,
                        segments: vec![(timestamp, tier.monthly_price_in_cents)],
                    });
                }
            }
            SponsorsActivityAction::TierChange => {
                let Some(tier) = activity.sponsors_tier.as_ref() else {
                    continue;
                };

                if let Some(open) = state.spells.iter_mut().find(|s| s.end.is_none()) {
                    open.segments.push((timestamp, tier.monthly_price_in_cents));
                }
            }
            SponsorsActivityAction::CancelledSponsorship => {
                if let Some(open) = state.spells.iter_mut().find(|s| s.end.is_none()) {
                    open.end = Some(timestamp);
                }
            }
            SponsorsActivityAction::Refund => {
                if let Some(tier) = activity.sponsors_tier.as_ref() {
                    if tier.is_one_time {
                        state.one_time_refunded_in_cents += tier.monthly_price_in_cents;
                    } else {
                        state.recurring_refunded_in_cents += tier.monthly_price_in_cents;
                    }
                }
            }
            SponsorsActivityAction::PendingChange
            | SponsorsActivityAction::SponsorMatchDisabled => {}
        }
    }

    let mut contributions = Vec::new();
    let mut refunds = Vec::new();

    for (key, state) in states {
        let Some(sponsor) = state.sponsor else {
            continue;
        };

        let account = state.public.then(|| sponsor.account());

        for (activity_id, timestamp, amount_in_cents) in state.one_time {
            contributions.push(Contribution {
                provider: Provider::GitHub,
                id: activity_id,
                key: key.clone(),
                account: account.clone(),

                recurring: false,
                interval: None,
                start: timestamp,
                end: Some(timestamp),

                amount_in_cents,
                monthly_in_cents: 0,
                paid_in_cents: amount_in_cents,
                months_paid: 1,

                native_currency: CURRENCY.to_string(),
                native_amount_in_cents: amount_in_cents,
                native_paid_in_cents: amount_in_cents,
            });
        }

        for spell in &state.spells {
            let (paid_in_cents, months_paid) = spell.evaluate(now);

            contributions.push(Contribution {
                provider: Provider::GitHub,
                id: spell.activity_id.clone(),
                key: key.clone(),
                account: account.clone(),

                recurring: true,
                interval: Some(super::Interval::Monthly),
                start: spell.start,
                end: spell.end,

                amount_in_cents: spell.monthly_in_cents(),
                monthly_in_cents: spell.monthly_in_cents(),
                paid_in_cents,
                months_paid,

                native_currency: CURRENCY.to_string(),
                native_amount_in_cents: spell.monthly_in_cents(),
                native_paid_in_cents: paid_in_cents,
            });
        }

        for (recurring, amount_in_cents) in [
            (false, state.one_time_refunded_in_cents),
            (true, state.recurring_refunded_in_cents),
        ] {
            if amount_in_cents > 0 {
                refunds.push(Refund {
                    key: key.clone(),
                    recurring,
                    amount_in_cents,

                    native_currency: CURRENCY.to_string(),
                    native_amount_in_cents: amount_in_cents,
                });
            }
        }
    }

    (contributions, refunds)
}

pub async fn collect(env: &crate::env::Env) -> Result<ProviderLedger, anyhow::Error> {
    let (activities, reported_monthly_in_cents) = collect_activities(env).await?;
    let (mut contributions, mut refunds) = build(&activities, chrono::Utc::now());

    let currency = env.sponsors_currency.as_str();
    if currency != CURRENCY {
        let rate = super::rate(env, CURRENCY, currency).await?;
        let convert = |cents: i64| (cents as f64 * rate).round() as i64;

        for contribution in &mut contributions {
            contribution.amount_in_cents = convert(contribution.amount_in_cents);
            contribution.monthly_in_cents = convert(contribution.monthly_in_cents);
            contribution.paid_in_cents = convert(contribution.paid_in_cents);
        }

        for refund in &mut refunds {
            refund.amount_in_cents = convert(refund.amount_in_cents);
        }
    }

    Ok(ProviderLedger {
        provider: Provider::GitHub,
        contributions,
        refunds,
        reported_monthly_in_cents: match reported_monthly_in_cents {
            Some(cents) => Some(super::convert(env, cents, CURRENCY, currency).await?),
            None => None,
        },
    })
}
