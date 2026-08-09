use super::State;
use utoipa_axum::{router::OpenApiRouter, routes};

mod get {
    use crate::{
        response::{ApiResponse, ApiResponseResult},
        routes::{ApiError, GetState},
        sponsors::{cached_ledger, evaluate},
    };
    use axum::http::StatusCode;
    use serde::Serialize;
    use utoipa::ToSchema;

    nestify::nest! {
        #[derive(ToSchema, Serialize)]
        struct Response {
            currency: &'static str,
            monthly_cents: i64,

            #[schema(inline)]
            totals: #[derive(ToSchema, Serialize)] struct ResponseTotals {
                one_time_cents: i64,
                recurring_cents: i64,
                lifetime_cents: i64,
            },

            sponsors: Vec<#[derive(ToSchema, Serialize)] struct ResponseSponsor {
                #[schema(inline)]
                status: #[derive(ToSchema, Serialize)] #[serde(rename_all = "snake_case")] enum ResponseSponsorStatus {
                    Monthly,
                    Former,
                    OneTime,
                },

                #[schema(inline)]
                profile: Option<#[derive(ToSchema, Serialize)] struct ResponseSponsorProfile {
                    github_id: Option<i64>,
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
        if state.env.github_token.is_none() {
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

        let evaluation = evaluate(&ledger, chrono::Utc::now());

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

                let public = sponsor.sponsor.is_some();

                ResponseSponsor {
                    status,
                    profile: sponsor.sponsor.map(|sponsor| ResponseSponsorProfile {
                        github_id: sponsor.database_id,
                        login: sponsor.login,
                        name: sponsor.name,
                        url: sponsor.url,
                        avatar_url: sponsor.avatar_url,
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
            currency: "USD",
            monthly_cents: evaluation.monthly_recurring_in_cents,

            totals: ResponseTotals {
                one_time_cents: evaluation.one_time_in_cents,
                recurring_cents: evaluation.recurring_in_cents,
                lifetime_cents: evaluation.lifetime_in_cents,
            },

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
