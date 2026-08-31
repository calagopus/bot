use super::State;
use utoipa_axum::{router::OpenApiRouter, routes};

mod get {
    use crate::{
        response::{ApiResponse, ApiResponseResult},
        routes::{ApiError, GetState},
    };
    use axum::http::StatusCode;
    use indexmap::IndexMap;
    use serde::Serialize;
    use serenity::all::GuildId;
    use std::sync::{Arc, LazyLock};
    use utoipa::ToSchema;

    nestify::nest! {
        #[derive(ToSchema, Serialize)]
        struct Response {
            id: String,
            name: String,

            #[schema(inline)]
            members: #[derive(ToSchema, Serialize)] struct ResponseMembers {
                #[schema(inline)]
                approximate_total: Option<u32>,
                #[schema(inline)]
                approximate_online: Option<u32>,
            },

            #[schema(inline)]
            channels: #[derive(ToSchema, Serialize)] struct ResponseChannels {
                total: usize,
                by_type: IndexMap<String, usize>,
            },

            #[schema(inline)]
            emojis: #[derive(ToSchema, Serialize)] struct ResponseEmojis {
                total: usize,
                animated: usize,
            },

            roles: usize,
            stickers: usize,

            #[schema(inline)]
            boosts: #[derive(ToSchema, Serialize)] struct ResponseBoosts {
                tier: u8,
                #[schema(inline)]
                count: Option<u32>,
            },
        }
    }

    static STATS_CACHE: LazyLock<moka::future::Cache<(), Arc<Response>>> = LazyLock::new(|| {
        moka::future::Cache::builder()
            .time_to_live(std::time::Duration::from_secs(60))
            .max_capacity(1)
            .build()
    });

    async fn collect_stats(state: &crate::routes::State) -> Result<Response, anyhow::Error> {
        let http = state.bot.read().await.clone();

        let guild_id = match state.env.guild_id {
            Some(guild_id) => GuildId::new(guild_id),
            None => {
                let Some(channel) = http
                    .get_channel(state.env.github_channel_id.into())
                    .await?
                    .guild()
                else {
                    return Err(anyhow::anyhow!(
                        "github channel ID {} is not a guild channel",
                        state.env.github_channel_id
                    ));
                };

                channel.base.guild_id
            }
        };

        let (guild, channels) = tokio::try_join!(
            http.get_guild_with_counts(guild_id),
            http.get_channels(guild_id)
        )?;

        let mut by_type: IndexMap<String, usize> = IndexMap::new();
        for channel in channels.iter() {
            *by_type
                .entry(channel.base.kind.name().to_string())
                .or_default() += 1;
        }
        by_type.sort_unstable_keys();

        Ok(Response {
            id: guild.id.get().to_string(),
            name: guild.name.to_string(),

            members: ResponseMembers {
                approximate_total: guild.approximate_member_count.map(|c| c.get()),
                approximate_online: guild.approximate_presence_count.map(|c| c.get()),
            },

            channels: ResponseChannels {
                total: channels.len(),
                by_type,
            },

            emojis: ResponseEmojis {
                total: guild.emojis.len(),
                animated: guild.emojis.iter().filter(|emoji| emoji.animated()).count(),
            },

            roles: guild.roles.len(),
            stickers: guild.stickers.len(),

            boosts: ResponseBoosts {
                tier: guild.premium_tier.0,
                count: guild.premium_subscription_count.map(|c| c.get()),
            },
        })
    }

    #[utoipa::path(get, path = "/", responses(
        (status = OK, body = inline(Response)),
        (status = SERVICE_UNAVAILABLE, body = ApiError),
    ))]
    pub async fn route(state: GetState) -> ApiResponseResult {
        let stats = match STATS_CACHE
            .try_get_with((), async { collect_stats(&state).await.map(Arc::new) })
            .await
        {
            Ok(stats) => stats,
            Err(err) => {
                tracing::error!("failed to fetch guild stats: {:?}", err);

                return ApiResponse::error("failed to fetch discord server data")
                    .with_status(StatusCode::SERVICE_UNAVAILABLE)
                    .ok();
            }
        };

        ApiResponse::json(&*stats).ok()
    }
}

pub fn router(state: &State) -> OpenApiRouter<State> {
    OpenApiRouter::new()
        .routes(routes!(get::route))
        .with_state(state.clone())
}
