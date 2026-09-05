use serenity::all::{
    ComponentInteraction, ComponentInteractionDataKind, CreateComponent, CreateContainer,
    CreateContainerComponent, CreateInteractionResponseFollowup, CreateTextDisplay,
    EditInteractionResponse, MessageFlags,
};

pub struct SponsorUnlink;

#[async_trait::async_trait]
impl crate::components::Component for SponsorUnlink {
    async fn execute(
        &self,
        state: &crate::routes::State,
        ctx: &serenity::prelude::Context,
        interaction: &ComponentInteraction,
    ) -> Result<Option<()>, anyhow::Error> {
        if !matches!(interaction.data.kind, ComponentInteractionDataKind::Button) {
            return Ok(None);
        }

        let Some(provider) = interaction
            .data
            .custom_id
            .strip_prefix(super::SPONSOR_UNLINK_ID)
            .and_then(crate::sponsors::Provider::from_key)
        else {
            return Ok(None);
        };

        interaction.defer(&ctx.http).await?;

        let unlinked = crate::models::unlink_discord_user(
            state.database.write(),
            provider,
            interaction.user.id.get() as i64,
        )
        .await?;

        if !unlinked.is_empty()
            && let Err(err) =
                crate::tiers::sync_linked_member(state, interaction.user.id.get()).await
        {
            tracing::error!(
                "failed to update sponsorship tier roles after unlinking: {:?}",
                err
            );
        }

        let linked = crate::models::find_linked_accounts_by_discord(
            state.database.read(),
            interaction.user.id.get() as i64,
        )
        .await?;

        interaction
            .edit_response(
                &ctx.http,
                EditInteractionResponse::new()
                    .components(crate::commands::sponsor::unlink_components(&linked))
                    .flags(MessageFlags::IS_COMPONENTS_V2),
            )
            .await?;

        let content = if unlinked.is_empty() {
            format!(
                "Your Discord account is not linked to a {} account.",
                provider.label()
            )
        } else {
            let logins = unlinked
                .iter()
                .filter_map(|account| account.login.as_deref())
                .collect::<Vec<_>>()
                .join(", ");

            format!(
                "Unlinked your {} account ({}).",
                provider.label(),
                if logins.is_empty() {
                    "unknown"
                } else {
                    &logins
                }
            )
        };

        interaction
            .create_followup(
                &ctx.http,
                CreateInteractionResponseFollowup::new()
                    .components(vec![CreateComponent::Container(CreateContainer::new(
                        vec![
                            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(
                                crate::commands::sponsor::HEADING_UNLINKED,
                            )),
                            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(content)),
                        ],
                    ))])
                    .flags(MessageFlags::IS_COMPONENTS_V2)
                    .ephemeral(true),
            )
            .await?;

        Ok(Some(()))
    }
}
