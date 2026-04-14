use serde::{Deserialize, Serialize};
use serenity::all::{
    CreateComponent, CreateContainer, CreateContainerComponent, CreateMessage, CreateSection,
    CreateSectionAccessory, CreateSectionComponent, CreateTextDisplay, CreateThumbnail,
    CreateUnfurledMediaItem, MessageFlags,
};

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubSponsor {
    pub login: String,
    pub name: Option<String>,
    pub avatar_url: String,
    pub url: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubSponsorsTier {
    pub monthly_price_in_cents: u64,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubSponsorshipNode {
    pub id: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub sponsor: GitHubSponsor,
    pub sponsors_tier: Option<GitHubSponsorsTier>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubSponsorsPageInfo {
    pub end_cursor: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubSponsorsActivities {
    pub nodes: Vec<GithubSponsorshipNode>,
    pub page_info: GitHubSponsorsPageInfo,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubSponsorsOrganization {
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
    pub data: GitHubSponsorsData,
}

fn graphql_query(env: &crate::env::Env, after: Option<&str>) -> String {
    let after = after
        .map(|c| format!(r#", after: "{c}""#))
        .unwrap_or("".to_string());

    let login = env.github_sponsors_login.as_deref().unwrap_or("calagopus");

    format!(
        r#"
        query {{
            organization(login: "{login}") {{
                sponsorsActivities(first: 100, period: ALL{after}) {{
                    nodes {{
                        id
                        timestamp
                        sponsorsTier {{
                            monthlyPriceInCents
                        }}
                        sponsor {{
                            ... on User {{
                                login
                                avatarUrl
                                name
                                url
                            }}
                            ... on Organization {{
                                login
                                avatarUrl
                                name
                                url
                            }}
                        }}
                    }}
                    pageInfo {{
                        endCursor
                    }}
                }}
            }}
        }}
        "#
    )
}

/// Old to new
pub async fn collect_sponsors(
    env: &crate::env::Env,
) -> Result<Vec<GithubSponsorshipNode>, anyhow::Error> {
    let Some(github_token) = &env.github_token else {
        return Ok(vec![]);
    };

    let client = reqwest::Client::builder()
        .user_agent(format!("Calagopus Bot ({})", crate::VERSION))
        .build()?;
    let mut sponsors = Vec::new();
    let mut after = None;

    loop {
        let res = client
            .post("https://api.github.com/graphql")
            .bearer_auth(github_token)
            .json(&serde_json::json!({ "query": graphql_query(env, after.as_deref()) }))
            .send()
            .await?
            .error_for_status()?;

        let data: GitHubSponsorsResponse = res.json().await?;
        sponsors.extend(data.data.organization.sponsors_activities.nodes);
        after = data
            .data
            .organization
            .sponsors_activities
            .page_info
            .end_cursor;

        if after.is_none() {
            break;
        }
    }

    sponsors.reverse();

    Ok(sponsors)
}

pub fn spawn_sponsor_updates_task(state: crate::routes::State) {
    tokio::spawn(async move {
        let Some(channel_id) = state.env.github_sponsors_channel_id else {
            return;
        };

        loop {
            let run_inner = async || -> Result<(), anyhow::Error> {
                let sponsors = collect_sponsors(&state.env).await?;

                for sponsor in sponsors {
                    if let Ok(sponsorship) =
                        crate::models::find_sent_sponsorship(state.database.read(), &sponsor.id)
                            .await
                    {
                        tracing::debug!(
                            "sponsorship {} already sent (from: {:?})",
                            sponsorship.id,
                            sponsorship.created
                        );
                        continue;
                    }

                    let monthly_price_in_dollars = sponsor
                        .sponsors_tier
                        .unwrap_or(GitHubSponsorsTier {
                            monthly_price_in_cents: 0,
                        })
                        .monthly_price_in_cents
                        as f64
                        / 100.0;

                    tracing::info!(
                        "new sponsor: {} ({}), tier: ${:.2}",
                        sponsor.sponsor.login,
                        sponsor.sponsor.url,
                        monthly_price_in_dollars
                    );

                    let mut container_components = Vec::new();

                    container_components.push(CreateContainerComponent::Section(CreateSection::new(
                        vec![
                            CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                                "## <:cash:1150889514236137605> Sponsorship received",
                            )),
                            CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                                    format!(
                                        "[**{login}**](https://github.com/{login}) sponsored us for `${:.2}`!",
                                        monthly_price_in_dollars,
                                        login = sponsor.sponsor.login
                                    )
                            )),
                        ],
                        CreateSectionAccessory::Thumbnail(CreateThumbnail::new(CreateUnfurledMediaItem::new(
                            sponsor.sponsor.avatar_url,
                        ))),
                    )));
                    container_components.push(CreateContainerComponent::TextDisplay(
                        CreateTextDisplay::new(format!(
                            "-# https://github.com/sponsors/{}",
                            state
                                .env
                                .github_sponsors_login
                                .as_deref()
                                .unwrap_or("calagopus")
                        )),
                    ));

                    let Some(channel) = state
                        .bot
                        .read()
                        .await
                        .get_channel(channel_id.into())
                        .await?
                        .guild()
                    else {
                        return Err(anyhow::anyhow!(
                            "github webhook channel ID {} is not a guild channel",
                            channel_id
                        ));
                    };

                    if !container_components.is_empty() {
                        let component =
                            CreateComponent::Container(CreateContainer::new(container_components));

                        channel
                            .send_message(
                                &*state.bot.read().await,
                                CreateMessage::new()
                                    .components(&[component])
                                    .flags(MessageFlags::IS_COMPONENTS_V2),
                            )
                            .await?;
                    }

                    crate::models::insert_sent_sponsorship(
                        state.database.write(),
                        &sponsor.id,
                        Some(sponsor.timestamp),
                    )
                    .await?;
                }

                Ok(())
            };

            if let Err(err) = run_inner().await {
                tracing::error!("failed to collect sponsors: {:?}", err);
                sentry_anyhow::capture_anyhow(&err);
            }

            tokio::time::sleep(std::time::Duration::from_mins(5)).await;
        }
    });
}
