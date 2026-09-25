use crate::{
    oauth,
    response::{ApiResponse, ApiResponseResult},
    routes::State,
    sponsors::Provider,
};
use axum::http::StatusCode;
use serde::Deserialize;
use serenity::all::{
    CreateComponent, CreateContainer, CreateContainerComponent, CreateInteractionResponseFollowup,
    CreateTextDisplay, EditInteractionResponse, MessageFlags, UserId,
};
use utoipa::{IntoParams, ToSchema};

/// Refreshes the `/sponsor link` message this started from and confirms below it. The
/// token only lives in memory, so after a restart the web page is the only confirmation.
async fn report_back(
    state: &State,
    request: &oauth::LinkRequest,
    user: &oauth::OauthUser,
) -> Result<(), anyhow::Error> {
    let linked = crate::models::find_linked_accounts_by_discord(
        state.database.read(),
        request.discord_user_id as i64,
    )
    .await?;

    let http = state.bot.read().await.clone();

    EditInteractionResponse::new()
        .components(
            crate::commands::sponsor::link_components(
                &state.env,
                &linked,
                request.discord_user_id,
                &request.interaction_token,
            )
            .await?,
        )
        .flags(MessageFlags::IS_COMPONENTS_V2)
        .execute(&http, &request.interaction_token)
        .await?;

    CreateInteractionResponseFollowup::new()
        .components(vec![CreateComponent::Container(CreateContainer::new(
            vec![
                CreateContainerComponent::TextDisplay(CreateTextDisplay::new(
                    crate::commands::sponsor::HEADING_LINKED,
                )),
                CreateContainerComponent::TextDisplay(CreateTextDisplay::new(format!(
                    "Linked **{}** on {}.",
                    user.login(),
                    request.provider.label(),
                ))),
            ],
        ))])
        .flags(MessageFlags::IS_COMPONENTS_V2)
        .ephemeral(true)
        .execute(&http, None, &request.interaction_token)
        .await?;

    Ok(())
}

fn page(heading: &str, message: &str) -> String {
    document(heading, &format!("<p>{message}</p>"))
}

fn confirm_page(
    heading: &str,
    message: &str,
    confirmation: &str,
    action: &str,
    token: &str,
) -> String {
    document(
        heading,
        &format!(
            r#"<p>{message}</p><form method="post" action="{action}"><input type="hidden" name="token" value="{token}"><label><input type="checkbox" required> {confirmation}</label><button type="submit">Link accounts</button></form>"#
        ),
    )
}

fn document(heading: &str, body: &str) -> String {
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
form {{ display: grid; gap: 1rem; justify-items: center; margin-top: 1.5rem; }}
button {{ font: inherit; padding: 0.5rem 1.25rem; cursor: pointer; }}
</style>
</head>
<body><main><h1>{heading}</h1>{body}</main></body>
</html>"#
    )
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[derive(Deserialize, IntoParams)]
pub struct Params {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

#[derive(Deserialize, ToSchema)]
pub struct ConfirmParams {
    pub token: Option<String>,
}

pub async fn callback(state: &State, provider: Provider, params: Params) -> ApiResponseResult {
    if !oauth::is_configured(&state.env, provider) {
        return ApiResponse::error("account linking is not configured")
            .with_status(StatusCode::SERVICE_UNAVAILABLE)
            .ok();
    }

    if let Some(error) = params.error {
        tracing::debug!(
            "{} oauth callback returned an error: {}",
            provider.key(),
            error
        );

        return ApiResponse::html(page(
            "Linking cancelled",
            &format!(
                "{} did not authorize the request. You can start over with `/sponsor link`.",
                provider.label()
            ),
        ))
        .with_status(StatusCode::BAD_REQUEST)
        .ok();
    }

    let (Some(code), Some(link_state)) = (params.code, params.state) else {
        return ApiResponse::error("missing code or state parameter")
            .with_status(StatusCode::BAD_REQUEST)
            .ok();
    };

    let Some(request) = oauth::take_link_state(provider, &link_state).await else {
        return ApiResponse::html(page(
            "Link request expired",
            "This link request is no longer valid. Run `/sponsor link` again to get a new one.",
        ))
        .with_status(StatusCode::BAD_REQUEST)
        .ok();
    };

    let user = oauth::exchange_code(&state.env, provider, &code).await?;

    let http = state.bot.read().await.clone();
    let discord_user = http.get_user(UserId::new(request.discord_user_id)).await?;

    let logins = user
        .accounts
        .iter()
        .map(|(_, login)| format!("<strong>{}</strong>", escape(login)))
        .collect::<Vec<_>>()
        .join(", ");
    let discord_tag = format!("<strong>@{}</strong>", escape(&discord_user.tag()));

    let token = oauth::create_pending_link(request, user).await;

    let mut response = ApiResponse::html(confirm_page(
        &format!("Link your {} account", provider.label()),
        &format!(
            "This links {} {logins} to the Discord account {discord_tag}. Any other Discord account it is linked to loses the link.",
            provider.label()
        ),
        &format!("{discord_tag} is my Discord account"),
        &escape(&format!(
            "{}/api/{}/oauth/confirm",
            state.env.app_url.trim_end_matches('/'),
            provider.key()
        )),
        &token,
    ));
    response.headers.insert(
        axum::http::header::X_FRAME_OPTIONS,
        axum::http::HeaderValue::from_static("DENY"),
    );

    response.ok()
}

pub async fn confirm(
    state: &State,
    provider: Provider,
    params: ConfirmParams,
) -> ApiResponseResult {
    if !oauth::is_configured(&state.env, provider) {
        return ApiResponse::error("account linking is not configured")
            .with_status(StatusCode::SERVICE_UNAVAILABLE)
            .ok();
    }

    let Some(token) = params.token else {
        return ApiResponse::error("missing token parameter")
            .with_status(StatusCode::BAD_REQUEST)
            .ok();
    };

    let Some(oauth::PendingLink { request, user }) =
        oauth::take_pending_link(provider, &token).await
    else {
        return ApiResponse::html(page(
            "Link request expired",
            "This link request is no longer valid. Run `/sponsor link` again to get a new one.",
        ))
        .with_status(StatusCode::BAD_REQUEST)
        .ok();
    };

    let discord_user_id = request.discord_user_id;

    let displaced_user_ids = crate::models::link_accounts(
        state.database.write(),
        provider,
        &user.accounts,
        discord_user_id as i64,
    )
    .await?;

    tracing::info!(
        "linked {} account {} to discord user {} ({} account(s))",
        provider.key(),
        user.login(),
        discord_user_id,
        user.accounts.len()
    );

    // Whoever lost an account here may have nothing linked left, and the periodic sync
    // only visits members that still have a link, so it would never take their role back.
    for displaced_user_id in displaced_user_ids {
        tracing::info!(
            "{} account(s) moved from discord user {} to discord user {}",
            provider.key(),
            displaced_user_id,
            discord_user_id
        );

        if let Err(err) = crate::tiers::sync_linked_member(state, displaced_user_id as u64).await {
            tracing::error!(
                "failed to update sponsorship tier roles for discord user {}: {:?}",
                displaced_user_id,
                err
            );
        }
    }

    if let Err(err) = crate::tiers::sync_linked_member(state, discord_user_id).await {
        tracing::error!(
            "failed to apply sponsorship tier roles after linking discord user {}: {:?}",
            discord_user_id,
            err
        );
    }

    if let Err(err) = report_back(state, &request, &user).await {
        tracing::debug!(
            "failed to report the link back to discord user {}: {:?}",
            discord_user_id,
            err
        );
    }

    ApiResponse::html(page(
        &format!("{} account linked", provider.label()),
        &format!("Your Discord account is now linked to {}.", user.login()),
    ))
    .ok()
}
