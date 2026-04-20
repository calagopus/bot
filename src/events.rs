use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
};

use serenity::all::{ActivityData, Event, Interaction, ReactionType, Timestamp, UserId};
use tokio::{sync::Mutex, time};

pub struct EventHandler {
    pub components: crate::components::ComponentList,
}

#[async_trait::async_trait]
impl serenity::all::RawEventHandler for EventHandler {
    async fn raw_event(&self, ctx: serenity::all::Context, event: &Event) {
        let state: crate::routes::State = ctx.data();

        let run = async || -> Result<(), anyhow::Error> {
            match event {
                Event::Ready(event) => {
                    if !state.env.app_debug {
                        poise::builtins::register_globally(
                            &ctx.http,
                            crate::commands::commands(crate::commands::CommandList::new())
                                .into_vec()
                                .iter(),
                        )
                        .await
                        .ok();
                    }

                    ctx.set_presence(
                        Some(ActivityData::custom("Playing with Rust Code")),
                        serenity::all::OnlineStatus::Idle,
                    );

                    tracing::info!(
                        user = %event.ready.user.name,
                        "bot connected"
                    );
                }
                Event::MessageCreate(event) => {
                    if event.message.author.bot() {
                        return Ok(());
                    }

                    if event.message.mentions_me(&ctx.http).await? {
                        event
                            .message
                            .react(&ctx.http, ReactionType::Unicode('👋'.into()))
                            .await?;
                    }

                    let Some(guild_id) = event.message.guild_id else {
                        return Ok(());
                    };

                    for mention in &event.message.mentions {
                        if state.env.antimention_user_ids.contains(&mention.id.get())
                            && !state
                                .env
                                .antimention_user_ids
                                .contains(&event.message.author.id.get())
                            && let Ok(mut member) =
                                guild_id.member(&ctx.http, event.message.author.id).await
                        {
                            for role_id in &state.env.antimention_whitelisted_role_ids {
                                if member.roles.contains(&(*role_id).into()) {
                                    return Ok(());
                                }
                            }

                            static TIMEOUT_MAP: LazyLock<Arc<Mutex<HashMap<UserId, u32>>>> =
                                LazyLock::new(|| {
                                    let map = Arc::new(Mutex::new(HashMap::new()));

                                    tokio::spawn({
                                        let map = Arc::clone(&map);
                                        async move {
                                            loop {
                                                time::sleep(time::Duration::from_hours(6)).await;
                                                let mut map = map.lock().await;
                                                map.retain(|_, count| *count > 0);
                                                for count in map.values_mut() {
                                                    if *count > 0 {
                                                        *count -= 1;
                                                    }
                                                }
                                            }
                                        }
                                    });

                                    map
                                });

                            let mut timeout_map = TIMEOUT_MAP.lock().await;
                            let timeout_count = timeout_map.entry(mention.id).or_insert(0);
                            *timeout_count += 1;
                            let timeout_duration = match *timeout_count {
                                1 | 2 => 30,
                                3 | 4 => 60,
                                5 => 300,
                                6 => 600,
                                _ => 3600,
                            };
                            drop(timeout_map);

                            let timestamp = match Timestamp::from_unix_timestamp(
                                chrono::Utc::now().timestamp() + timeout_duration,
                            ) {
                                Ok(t) => t,
                                Err(_) => return Ok(()),
                            };

                            member
                                .disable_communication_until(&ctx.http, timestamp)
                                .await?;
                            event
                                .message
                                .reply_ping(&ctx.http, "👋 Hey, please do not mention this person. You have been temporarily timed out, repeated offenses will result in longer timeouts.")
                                .await?;

                            break;
                        }
                    }
                }
                Event::InteractionCreate(event) => {
                    if let Interaction::Component(component) = &event.interaction
                        && self
                            .components
                            .execute_component(&state, &ctx, component)
                            .await?
                            .is_none()
                    {
                        tracing::warn!(
                            interaction_id = %component.id,
                            "no component found for interaction"
                        );
                    }
                }
                _ => {}
            }

            Ok(())
        };

        if let Err(err) = run().await {
            tracing::error!("Error handling event: {:#?}", err);
            sentry_anyhow::capture_anyhow(&err);
        }
    }
}
