use crate::sponsors::Provider;
use serde::Deserialize;
use std::sync::LazyLock;

#[derive(Deserialize)]
struct AccessTokenResponse {
    access_token: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

#[derive(Deserialize, Debug)]
struct GitHubOauthUser {
    id: i64,
    login: String,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct OpenCollectiveAccount {
    id: String,
    slug: String,
}

#[derive(Deserialize, Debug)]
struct OpenCollectiveMemberOfNode {
    account: Option<OpenCollectiveAccount>,
}

#[derive(Deserialize, Debug)]
struct OpenCollectiveMemberOf {
    nodes: Vec<OpenCollectiveMemberOfNode>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct OpenCollectiveMe {
    id: String,
    slug: String,
    member_of: Option<OpenCollectiveMemberOf>,
}

#[derive(Deserialize, Debug)]
struct OpenCollectiveMeData {
    me: Option<OpenCollectiveMe>,
}

#[derive(Deserialize)]
struct OpenCollectiveResponse<T> {
    data: Option<T>,
    errors: Option<Vec<serde_json::Value>>,
}

#[derive(Debug)]
pub struct OauthUser {
    /// `(provider id, login)` pairs, all linked together.
    pub accounts: Vec<(String, String)>,
}

impl OauthUser {
    #[inline]
    pub fn login(&self) -> &str {
        self.accounts
            .first()
            .map(|(_, login)| login.as_str())
            .unwrap_or("unknown")
    }
}

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .user_agent(format!("Calagopus Bot ({})", crate::VERSION))
        .build()
        .expect("failed to build oauth http client")
});

#[derive(Debug, Clone)]
pub struct LinkRequest {
    pub provider: Provider,
    pub discord_user_id: u64,
    /// Token of the `/sponsor link` interaction, so the outcome can be reported back to
    /// it. Discord accepts followups on it for 15 minutes, longer than the state lives.
    pub interaction_token: String,
}

static LINK_STATES: LazyLock<moka::future::Cache<String, LinkRequest>> = LazyLock::new(|| {
    moka::future::Cache::builder()
        .time_to_live(std::time::Duration::from_mins(10))
        .max_capacity(1024)
        .build()
});

#[inline]
pub fn is_configured(env: &crate::env::Env, provider: Provider) -> bool {
    credentials(env, provider).is_some()
}

#[inline]
fn credentials(env: &crate::env::Env, provider: Provider) -> Option<(&str, &str)> {
    let (client_id, client_secret) = match provider {
        Provider::GitHub => (
            env.github_oauth_client_id.as_deref(),
            env.github_oauth_client_secret.as_deref(),
        ),
        Provider::OpenCollective => (
            env.opencollective_oauth_client_id.as_deref(),
            env.opencollective_oauth_client_secret.as_deref(),
        ),
    };

    Some((client_id?, client_secret?))
}

#[inline]
pub fn redirect_uri(env: &crate::env::Env, provider: Provider) -> String {
    format!(
        "{}/api/{}/oauth/callback",
        env.app_url.trim_end_matches('/'),
        provider.key()
    )
}

pub async fn create_link_state(
    provider: Provider,
    discord_user_id: u64,
    interaction_token: &str,
) -> String {
    let mut bytes = [0u8; 32];
    rand::fill(&mut bytes);

    let state = hex::encode(bytes);
    LINK_STATES
        .insert(
            state.clone(),
            LinkRequest {
                provider,
                discord_user_id,
                interaction_token: interaction_token.to_string(),
            },
        )
        .await;

    state
}

pub async fn take_link_state(provider: Provider, state: &str) -> Option<LinkRequest> {
    LINK_STATES
        .remove(state)
        .await
        .filter(|request| request.provider == provider)
}

pub fn authorize_url(
    env: &crate::env::Env,
    provider: Provider,
    state: &str,
) -> Result<String, anyhow::Error> {
    let Some((client_id, _)) = credentials(env, provider) else {
        return Err(anyhow::anyhow!(
            "{} oauth is not configured",
            provider.key()
        ));
    };

    let endpoint = match provider {
        Provider::GitHub => "https://github.com/login/oauth/authorize",
        Provider::OpenCollective => "https://opencollective.com/oauth/authorize",
    };

    // No scope is requested: both providers hand out the public identity the linking
    // needs (`id`, `login` and the collectives someone administers) to an unscoped token.
    let url = reqwest::Url::parse_with_params(
        endpoint,
        &[
            ("client_id", client_id),
            ("redirect_uri", &redirect_uri(env, provider)),
            ("response_type", "code"),
            ("state", state),
        ],
    )?;

    Ok(url.into())
}

async fn access_token(
    env: &crate::env::Env,
    provider: Provider,
    code: &str,
) -> Result<String, anyhow::Error> {
    let Some((client_id, client_secret)) = credentials(env, provider) else {
        return Err(anyhow::anyhow!(
            "{} oauth is not configured",
            provider.key()
        ));
    };

    let redirect_uri = redirect_uri(env, provider);

    let request = match provider {
        Provider::GitHub => CLIENT
            .post("https://github.com/login/oauth/access_token")
            .header("Accept", "application/json")
            .json(&serde_json::json!({
                "client_id": client_id,
                "client_secret": client_secret,
                "code": code,
                "redirect_uri": redirect_uri,
            })),
        Provider::OpenCollective => CLIENT
            .post("https://opencollective.com/oauth/token")
            .header("Accept", "application/json")
            .form(&[
                ("grant_type", "authorization_code"),
                ("client_id", client_id),
                ("client_secret", client_secret),
                ("code", code),
                ("redirect_uri", &redirect_uri),
            ]),
    };

    let token: AccessTokenResponse = request.send().await?.error_for_status()?.json().await?;

    token.access_token.ok_or_else(|| {
        anyhow::anyhow!(
            "{} did not return an access token: {}",
            provider.key(),
            token
                .error_description
                .or(token.error)
                .unwrap_or_else(|| "unknown error".to_string())
        )
    })
}

const OPENCOLLECTIVE_ME_QUERY: &str = r#"
query {
    me {
        id
        slug
        memberOf(role: ADMIN, limit: 100) {
            nodes {
                account {
                    id
                    slug
                }
            }
        }
    }
}
"#;

pub async fn exchange_code(
    env: &crate::env::Env,
    provider: Provider,
    code: &str,
) -> Result<OauthUser, anyhow::Error> {
    let access_token = access_token(env, provider, code).await?;

    match provider {
        Provider::GitHub => {
            let user: GitHubOauthUser = CLIENT
                .get("https://api.github.com/user")
                .header("Accept", "application/vnd.github+json")
                .bearer_auth(access_token)
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;

            Ok(OauthUser {
                accounts: vec![(user.id.to_string(), user.login)],
            })
        }
        Provider::OpenCollective => {
            let response: OpenCollectiveResponse<OpenCollectiveMeData> = CLIENT
                .post(crate::sponsors::opencollective::GRAPHQL_URL)
                .bearer_auth(access_token)
                .json(&serde_json::json!({ "query": OPENCOLLECTIVE_ME_QUERY }))
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;

            if let Some(errors) = response.errors.filter(|errors| !errors.is_empty()) {
                return Err(anyhow::anyhow!(
                    "open collective graphql errors: {:?}",
                    errors
                ));
            }

            let Some(me) = response.data.and_then(|data| data.me) else {
                return Err(anyhow::anyhow!(
                    "open collective did not return the authorized account"
                ));
            };

            // Contributions are often made through an organisation rather than the
            // personal profile, so every account the member administers is linked too.
            let mut accounts = vec![(me.id, me.slug)];

            for node in me
                .member_of
                .map(|member_of| member_of.nodes)
                .unwrap_or_default()
            {
                let Some(account) = node.account else {
                    continue;
                };

                if accounts.iter().any(|(id, _)| *id == account.id) {
                    continue;
                }

                accounts.push((account.id, account.slug));
            }

            Ok(OauthUser { accounts })
        }
    }
}
