use serde::Deserialize;
use std::sync::LazyLock;

#[derive(Deserialize)]
struct GitHubAccessTokenResponse {
    access_token: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct GitHubOauthUser {
    pub id: i64,
    pub login: String,
}

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .user_agent(format!("Calagopus Bot ({})", crate::VERSION))
        .build()
        .expect("failed to build oauth http client")
});

static LINK_STATES: LazyLock<moka::future::Cache<String, u64>> = LazyLock::new(|| {
    moka::future::Cache::builder()
        .time_to_live(std::time::Duration::from_mins(10))
        .max_capacity(1024)
        .build()
});

#[inline]
pub fn is_configured(env: &crate::env::Env) -> bool {
    env.github_oauth_client_id.is_some() && env.github_oauth_client_secret.is_some()
}

#[inline]
pub fn redirect_uri(env: &crate::env::Env) -> String {
    format!(
        "{}/api/github/oauth/callback",
        env.app_url.trim_end_matches('/')
    )
}

pub async fn create_link_state(discord_user_id: u64) -> String {
    let mut bytes = [0u8; 32];
    rand::fill(&mut bytes);

    let state = hex::encode(bytes);
    LINK_STATES.insert(state.clone(), discord_user_id).await;

    state
}

pub async fn take_link_state(state: &str) -> Option<u64> {
    LINK_STATES.remove(state).await
}

pub fn authorize_url(env: &crate::env::Env, state: &str) -> Result<String, anyhow::Error> {
    let Some(client_id) = env.github_oauth_client_id.as_deref() else {
        return Err(anyhow::anyhow!("GITHUB_OAUTH_CLIENT_ID is not configured"));
    };

    let url = reqwest::Url::parse_with_params(
        "https://github.com/login/oauth/authorize",
        &[
            ("client_id", client_id),
            ("redirect_uri", &redirect_uri(env)),
            ("scope", "read:user"),
            ("state", state),
        ],
    )?;

    Ok(url.into())
}

pub async fn exchange_code(
    env: &crate::env::Env,
    code: &str,
) -> Result<GitHubOauthUser, anyhow::Error> {
    let (Some(client_id), Some(client_secret)) = (
        env.github_oauth_client_id.as_deref(),
        env.github_oauth_client_secret.as_deref(),
    ) else {
        return Err(anyhow::anyhow!("github oauth is not configured"));
    };

    let token: GitHubAccessTokenResponse = CLIENT
        .post("https://github.com/login/oauth/access_token")
        .header("Accept", "application/json")
        .json(&serde_json::json!({
            "client_id": client_id,
            "client_secret": client_secret,
            "code": code,
            "redirect_uri": redirect_uri(env),
        }))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let Some(access_token) = token.access_token else {
        return Err(anyhow::anyhow!(
            "github did not return an access token: {}",
            token
                .error_description
                .or(token.error)
                .unwrap_or_else(|| "unknown error".to_string())
        ));
    };

    let user: GitHubOauthUser = CLIENT
        .get("https://api.github.com/user")
        .header("Accept", "application/vnd.github+json")
        .bearer_auth(access_token)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    Ok(user)
}
