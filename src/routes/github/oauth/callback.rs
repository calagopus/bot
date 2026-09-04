use super::State;
use utoipa_axum::{router::OpenApiRouter, routes};

mod get {
    use crate::{
        oauth,
        response::{ApiResponse, ApiResponseResult},
        routes::{ApiError, GetState},
    };
    use axum::{extract::Query, http::StatusCode};
    use serde::Deserialize;
    use utoipa::IntoParams;

    fn page(heading: &str, message: &str) -> String {
        format!(
            r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{heading}</title>
<style>
:root {{ color-scheme: light dark; }}
body {{ margin: 0; min-height: 100vh; display: grid; place-items: center; font-family: system-ui, sans-serif; }}
main {{ max-width: 32rem; padding: 2rem; text-align: center; }}
h1 {{ font-size: 1.25rem; margin: 0 0 0.5rem; }}
p {{ margin: 0; opacity: 0.75; }}
</style>
</head>
<body><main><h1>{heading}</h1><p>{message}</p></main></body>
</html>"#
        )
    }

    #[derive(Deserialize, IntoParams)]
    pub struct Params {
        code: Option<String>,
        state: Option<String>,
        error: Option<String>,
    }

    #[utoipa::path(get, path = "/", params(Params), responses(
        (status = OK, body = String, content_type = "text/html"),
        (status = BAD_REQUEST, body = ApiError),
        (status = SERVICE_UNAVAILABLE, body = ApiError),
    ))]
    pub async fn route(state: GetState, Query(params): Query<Params>) -> ApiResponseResult {
        if !oauth::is_configured(&state.env) {
            return ApiResponse::error("github account linking is not configured")
                .with_status(StatusCode::SERVICE_UNAVAILABLE)
                .ok();
        }

        if let Some(error) = params.error {
            tracing::debug!("github oauth callback returned an error: {}", error);

            return ApiResponse::html(page(
                "Linking cancelled",
                "GitHub did not authorize the request. You can start over with /github link.",
            ))
            .with_status(StatusCode::BAD_REQUEST)
            .ok();
        }

        let (Some(code), Some(link_state)) = (params.code, params.state) else {
            return ApiResponse::error("missing code or state parameter")
                .with_status(StatusCode::BAD_REQUEST)
                .ok();
        };

        let Some(discord_user_id) = oauth::take_link_state(&link_state).await else {
            return ApiResponse::html(page(
                "Link request expired",
                "This link request is no longer valid. Run /github link again to get a new one.",
            ))
            .with_status(StatusCode::BAD_REQUEST)
            .ok();
        };

        let user = oauth::exchange_code(&state.env, &code).await?;

        crate::models::link_github_user(
            state.database.write(),
            user.id,
            &user.login,
            discord_user_id as i64,
        )
        .await?;

        tracing::info!(
            "linked github account {} ({}) to discord user {}",
            user.login,
            user.id,
            discord_user_id
        );

        if let Err(err) = crate::tiers::sync_linked_member(&state, user.id, discord_user_id).await {
            tracing::error!(
                "failed to apply sponsorship tier roles after linking discord user {}: {:?}",
                discord_user_id,
                err
            );
        }

        ApiResponse::html(page(
            "GitHub account linked",
            &format!("Your Discord account is now linked to {}.", user.login),
        ))
        .ok()
    }
}

pub fn router(state: &State) -> OpenApiRouter<State> {
    OpenApiRouter::new()
        .routes(routes!(get::route))
        .with_state(state.clone())
}
