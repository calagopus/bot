use serenity::all::{
    ButtonStyle, ComponentInteraction, ComponentInteractionDataKind, CreateActionRow, CreateButton,
    CreateComponent, CreateContainer, CreateContainerComponent, CreateInteractionResponse,
    CreateInteractionResponseMessage, EditThread, ForumTagId, MessageFlags,
};

const HEADING: &str = "## <:hammer:1150889684227076227> AI support";

pub fn control_components<'a>(thread: &crate::models::AiSupportThread) -> Vec<CreateComponent<'a>> {
    let body = if thread.resolved {
        "This post is marked as resolved and locked."
    } else if thread.enabled {
        "AI support is **on** for this post. It reads the documentation and answers with quotes checked word for word against it; every answer is still worth a second look.
Answering here as a team member turns it off again."
    } else {
        "AI support is **off** for this post. The author or the support team can turn it on to get a documentation-backed answer while they wait."
    };

    let mut components = vec![
        CreateContainerComponent::TextDisplay(serenity::all::CreateTextDisplay::new(HEADING)),
        CreateContainerComponent::TextDisplay(serenity::all::CreateTextDisplay::new(body)),
    ];

    components.push(CreateContainerComponent::ActionRow(
        CreateActionRow::Buttons(
            vec![
                CreateButton::new(format!(
                    "{}{}",
                    super::AI_SUPPORT_TOGGLE_ID,
                    thread.thread_id
                ))
                .label(if thread.enabled {
                    "Disable AI support"
                } else {
                    "Enable AI support"
                })
                .style(if thread.enabled {
                    ButtonStyle::Secondary
                } else {
                    ButtonStyle::Success
                })
                .disabled(thread.resolved),
                CreateButton::new(format!(
                    "{}{}",
                    super::AI_SUPPORT_RESOLVE_ID,
                    thread.thread_id
                ))
                .label("Mark resolved")
                .style(ButtonStyle::Secondary)
                .disabled(thread.resolved),
            ]
            .into(),
        ),
    ));

    vec![CreateComponent::Container(CreateContainer::new(components))]
}

async fn respond(
    ctx: &serenity::prelude::Context,
    interaction: &ComponentInteraction,
    content: &str,
) -> Result<(), anyhow::Error> {
    interaction
        .create_response(
            &ctx.http,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content(content.to_string())
                    .ephemeral(true),
            ),
        )
        .await?;

    Ok(())
}

fn thread_id(custom_id: &str, prefix: &str) -> Option<i64> {
    custom_id.strip_prefix(prefix)?.parse().ok()
}

pub struct AiSupportToggle;

#[async_trait::async_trait]
impl crate::components::Component for AiSupportToggle {
    async fn execute(
        &self,
        state: &crate::routes::State,
        ctx: &serenity::prelude::Context,
        interaction: &ComponentInteraction,
    ) -> Result<Option<()>, anyhow::Error> {
        if !matches!(interaction.data.kind, ComponentInteractionDataKind::Button) {
            return Ok(None);
        }

        let Some(thread_id) = thread_id(&interaction.data.custom_id, super::AI_SUPPORT_TOGGLE_ID)
        else {
            return Ok(None);
        };

        let Some(mut thread) =
            crate::models::find_ai_support_thread(state.database.read(), thread_id).await?
        else {
            respond(ctx, interaction, "This post is not tracked for AI support.").await?;
            return Ok(Some(()));
        };

        let author = thread.owner_id == Some(interaction.user.id.get() as i64);
        let staff = !author
            && interaction
                .member
                .as_ref()
                .is_some_and(|member| crate::ai::is_staff(&state.env, &member.roles));

        if !author && !staff {
            respond(
                ctx,
                interaction,
                "Only the author of this post or the support team can change this.",
            )
            .await?;
            return Ok(Some(()));
        }

        if thread.resolved {
            respond(ctx, interaction, "This post is already resolved.").await?;
            return Ok(Some(()));
        }

        thread.enabled = !thread.enabled;
        thread.paused = false;
        crate::models::set_ai_support_enabled(state.database.write(), thread_id, thread.enabled)
            .await?;

        interaction
            .create_response(
                &ctx.http,
                CreateInteractionResponse::UpdateMessage(
                    CreateInteractionResponseMessage::new()
                        .components(control_components(&thread))
                        .flags(MessageFlags::IS_COMPONENTS_V2),
                ),
            )
            .await?;

        tracing::info!(
            thread = thread_id,
            user = %interaction.user.name,
            enabled = thread.enabled,
            "ai support toggled"
        );

        if thread.enabled {
            crate::ai::spawn_enabled_answer(state, thread_id);
        }

        Ok(Some(()))
    }
}

pub struct AiSupportResolve;

#[async_trait::async_trait]
impl crate::components::Component for AiSupportResolve {
    async fn execute(
        &self,
        state: &crate::routes::State,
        ctx: &serenity::prelude::Context,
        interaction: &ComponentInteraction,
    ) -> Result<Option<()>, anyhow::Error> {
        if !matches!(interaction.data.kind, ComponentInteractionDataKind::Button) {
            return Ok(None);
        }

        let Some(thread_id) = thread_id(&interaction.data.custom_id, super::AI_SUPPORT_RESOLVE_ID)
        else {
            return Ok(None);
        };

        let Some(mut thread) =
            crate::models::find_ai_support_thread(state.database.read(), thread_id).await?
        else {
            respond(ctx, interaction, "This post is not tracked for AI support.").await?;
            return Ok(Some(()));
        };

        if !interaction
            .member
            .as_ref()
            .is_some_and(|member| crate::ai::is_staff(&state.env, &member.roles))
        {
            respond(
                ctx,
                interaction,
                "Only the support team can mark a post as resolved.",
            )
            .await?;
            return Ok(Some(()));
        }

        if thread.resolved {
            respond(ctx, interaction, "This post is already resolved.").await?;
            return Ok(Some(()));
        }

        thread.resolved = true;
        crate::models::set_ai_support_resolved(state.database.write(), thread_id, true).await?;

        interaction
            .create_response(
                &ctx.http,
                CreateInteractionResponse::UpdateMessage(
                    CreateInteractionResponseMessage::new()
                        .components(control_components(&thread))
                        .flags(MessageFlags::IS_COMPONENTS_V2),
                ),
            )
            .await?;

        let Some(guild_thread) = ctx.http.get_channel(interaction.channel_id).await?.thread()
        else {
            return Ok(Some(()));
        };

        let mut tags: Vec<ForumTagId> = guild_thread
            .applied_tags
            .iter()
            .copied()
            .filter(|tag| {
                state.env.ai_support_unresolved_tag_id != Some(tag.get())
                    && state.env.ai_support_resolved_tag_id != Some(tag.get())
            })
            .collect();
        if let Some(resolved_tag_id) = state.env.ai_support_resolved_tag_id {
            tags.push(ForumTagId::new(resolved_tag_id));
        }

        EditThread::new()
            .applied_tags(tags)
            .locked(true)
            .audit_log_reason("marked resolved")
            .execute(&ctx.http, guild_thread.id)
            .await?;

        tracing::info!(
            thread = thread_id,
            user = %interaction.user.name,
            "ai support thread resolved"
        );

        Ok(Some(()))
    }
}
