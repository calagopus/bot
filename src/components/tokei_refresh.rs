use crate::commands::tokei::{MessagePart, Refresh};
use serenity::all::{
    ComponentInteraction, ComponentInteractionDataKind, CreateInteractionResponseFollowup,
    EditInteractionResponse, MessageFlags,
};

pub struct TokeiRefresh;

#[async_trait::async_trait]
impl crate::components::Component for TokeiRefresh {
    async fn execute(
        &self,
        state: &crate::routes::State,
        ctx: &serenity::prelude::Context,
        interaction: &ComponentInteraction,
    ) -> Result<Option<()>, anyhow::Error> {
        if !matches!(interaction.data.kind, ComponentInteractionDataKind::Button) {
            return Ok(None);
        }

        let Some((repository, fetched_at)) = interaction
            .data
            .custom_id
            .strip_prefix(super::TOKEI_REFRESH_ID)
            .and_then(|data| data.rsplit_once(':'))
            .and_then(|(repository, fetched_at)| Some((repository, fetched_at.parse().ok()?)))
        else {
            return Ok(None);
        };

        interaction.defer(&ctx.http).await?;

        let refresh =
            match crate::commands::tokei::refresh(&state.env, repository, fetched_at).await {
                Ok(refresh) => refresh,
                Err(err) => {
                    tracing::error!(
                        "failed to refresh tokei stats for {}: {:?}",
                        repository,
                        err
                    );

                    followup(
                        ctx,
                        interaction,
                        "Failed to refresh the statistics, try again later.".to_string(),
                    )
                    .await?;

                    return Ok(Some(()));
                }
            };

        match refresh {
            Some(Refresh::Updated(stats)) => {
                let response = crate::commands::tokei::message_components(&stats)
                    .into_iter()
                    .fold(
                        EditInteractionResponse::new().clear_attachments(),
                        |response, part| match part {
                            MessagePart::Components(components) => response.components(components),
                            MessagePart::Attachment(attachment) => {
                                response.new_attachment(attachment)
                            }
                        },
                    )
                    .flags(MessageFlags::IS_COMPONENTS_V2);

                interaction.edit_response(&ctx.http, response).await?;
            }
            Some(Refresh::Cooldown {
                fetched_at,
                available_at,
            }) => {
                followup(
                    ctx,
                    interaction,
                    format!(
                        "These statistics were counted <t:{fetched_at}:R>, they can be refreshed again <t:{available_at}:R>."
                    ),
                )
                .await?;
            }
            None => {
                followup(
                    ctx,
                    interaction,
                    "This repository is no longer available.".to_string(),
                )
                .await?;
            }
        }

        Ok(Some(()))
    }
}

async fn followup(
    ctx: &serenity::prelude::Context,
    interaction: &ComponentInteraction,
    content: String,
) -> Result<(), anyhow::Error> {
    interaction
        .create_followup(
            &ctx.http,
            CreateInteractionResponseFollowup::new()
                .content(content)
                .ephemeral(true),
        )
        .await?;

    Ok(())
}
