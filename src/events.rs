use serenity::all::{ActivityData, Event, Interaction, ReactionType};

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
