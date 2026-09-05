use super::State;
use utoipa_axum::{router::OpenApiRouter, routes};

mod get {
    use crate::{
        response::{ApiResponse, ApiResponseResult},
        routes::{ApiError, GetState},
        sponsors::{Provider, cached_ledger, evaluate},
    };
    use axum::http::StatusCode;
    use serde::Serialize;
    use std::collections::HashMap;
    use utoipa::ToSchema;

    nestify::nest! {
        #[derive(ToSchema, Serialize)]
        struct Response {
            currency: String,
            monthly_cents: i64,

            #[schema(inline)]
            totals: #[derive(ToSchema, Serialize)] struct ResponseTotals {
                one_time_cents: i64,
                recurring_cents: i64,
                lifetime_cents: i64,
            },

            sponsors: Vec<#[derive(ToSchema, Serialize)] struct ResponseSponsor {
                provider: Provider,

                #[schema(inline)]
                status: #[derive(ToSchema, Serialize)] #[serde(rename_all = "snake_case")] enum ResponseSponsorStatus {
                    Monthly,
                    Former,
                    OneTime,
                },

                #[schema(inline)]
                profile: Option<#[derive(ToSchema, Serialize)] struct ResponseSponsorProfile {
                    account_id: String,
                    /// Only set for GitHub sponsors, kept for consumers written before
                    /// Open Collective support was added.
                    github_id: Option<i64>,
                    #[schema(inline)]
                    discord_user_id: Option<String>,
                    login: String,
                    name: Option<String>,
                    url: String,
                    avatar_url: String,
                }>,

                monthly_cents: i64,
                one_time_cents: i64,
                recurring_cents: i64,
                lifetime_cents: i64,

                #[schema(inline)]
                estimated_months: Option<u32>,

                #[schema(inline)]
                first_sponsored_at: Option<chrono::DateTime<chrono::Utc>>,
                #[schema(inline)]
                last_activity_at: Option<chrono::DateTime<chrono::Utc>>,
            }>,
        }
    }

    #[utoipa::path(get, path = "/", responses(
        (status = OK, body = inline(Response)),
        (status = SERVICE_UNAVAILABLE, body = ApiError),
    ))]
    pub async fn route(state: GetState) -> ApiResponseResult {
        if !crate::sponsors::is_configured(&state.env) {
            return ApiResponse::error("sponsor data is not configured")
                .with_status(StatusCode::SERVICE_UNAVAILABLE)
                .ok();
        }

        let ledger = match cached_ledger(&state.env).await {
            Ok(ledger) => ledger,
            Err(err) => {
                tracing::error!("failed to fetch sponsors: {:?}", err);

                return ApiResponse::error("failed to fetch sponsor data")
                    .with_status(StatusCode::SERVICE_UNAVAILABLE)
                    .ok();
            }
        };

        let evaluation = evaluate(&ledger);

        let linked: HashMap<(String, String), i64> =
            crate::models::linked_accounts(state.database.read())
                .await?
                .into_iter()
                .filter_map(|account| {
                    Some((
                        (account.provider, account.provider_id),
                        account.discord_user_id?,
                    ))
                })
                .collect();

        let mut sponsors: Vec<ResponseSponsor> = evaluation
            .sponsors
            .into_iter()
            .map(|sponsor| {
                let status = if sponsor.active {
                    ResponseSponsorStatus::Monthly
                } else if sponsor.recurring_spells > 0 {
                    ResponseSponsorStatus::Former
                } else {
                    ResponseSponsorStatus::OneTime
                };

                let public = sponsor.account.is_some();
                let discord_user_id = linked
                    .get(&(sponsor.provider.key().to_string(), sponsor.key.clone()))
                    .map(|discord_user_id| (*discord_user_id as u64).to_string());

                ResponseSponsor {
                    provider: sponsor.provider,
                    status,
                    profile: sponsor.account.map(|account| ResponseSponsorProfile {
                        github_id: (account.provider == Provider::GitHub)
                            .then(|| account.id.parse().ok())
                            .flatten(),
                        account_id: account.id,
                        discord_user_id,
                        login: account.login,
                        name: account.name,
                        url: account.url,
                        avatar_url: account.avatar_url,
                    }),

                    monthly_cents: sponsor.monthly_in_cents,
                    one_time_cents: sponsor.one_time_in_cents,
                    recurring_cents: sponsor.recurring_in_cents,
                    lifetime_cents: sponsor.lifetime_in_cents,

                    estimated_months: (sponsor.recurring_spells > 0)
                        .then_some(sponsor.estimated_months_paid),

                    first_sponsored_at: public.then_some(sponsor.first_sponsored_at),
                    last_activity_at: public.then_some(sponsor.last_activity_at),
                }
            })
            .collect();

        sponsors.sort_by_key(|sponsor| match sponsor.status {
            ResponseSponsorStatus::Monthly => 0,
            ResponseSponsorStatus::Former => 1,
            ResponseSponsorStatus::OneTime => 2,
        });

        ApiResponse::json(Response {
            monthly_cents: evaluation.monthly_recurring_in_cents,

            totals: ResponseTotals {
                one_time_cents: evaluation.one_time_in_cents,
                recurring_cents: evaluation.recurring_in_cents,
                lifetime_cents: evaluation.lifetime_in_cents,
            },

            currency: evaluation.currency,
            sponsors,
        })
        .ok()
    }
}

pub fn router(state: &State) -> OpenApiRouter<State> {
    OpenApiRouter::new()
        .routes(routes!(get::route))
        .with_state(state.clone())
}
