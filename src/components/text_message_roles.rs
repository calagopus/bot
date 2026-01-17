use serenity::all::{
    ComponentInteraction, ComponentInteractionDataKind, CreateInteractionResponse,
    CreateInteractionResponseMessage, EditInteractionResponse,
};

pub struct TextMessageRoles;

#[async_trait::async_trait]
impl crate::components::Component for TextMessageRoles {
    async fn execute(
        &self,
        state: &crate::routes::State,
        ctx: &serenity::prelude::Context,
        interaction: &ComponentInteraction,
    ) -> Result<Option<()>, anyhow::Error> {
        let ComponentInteractionDataKind::StringSelect { values, .. } = &interaction.data.kind
        else {
            return Ok(None);
        };

        let Some(text_message_id) = interaction
            .data
            .custom_id
            .strip_prefix("text_message_roles_select:")
        else {
            return Ok(None);
        };
        let (Some(guild_id), Ok(text_message_id)) =
            (interaction.guild_id, text_message_id.parse::<i64>())
        else {
            return Ok(None);
        };

        let Some(text_message): Option<crate::models::TextMessage> =
            sqlx::query_as("SELECT * FROM text_messages WHERE id = ?")
                .bind(text_message_id)
                .fetch_optional(state.database.write())
                .await?
        else {
            interaction
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("Text message not found.")
                            .ephemeral(true),
                    ),
                )
                .await?;
            return Ok(Some(()));
        };
        if text_message.channel_id != interaction.channel_id.get() as i64 {
            interaction
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("Invalid data selected.")
                            .ephemeral(true),
                    ),
                )
                .await?;
            return Ok(None);
        }

        interaction.defer_ephemeral(&ctx.http).await?;

        for (role_id, _) in text_message.roles {
            if values.contains(&role_id.to_string()) {
                ctx.http
                    .add_member_role(guild_id, interaction.user.id, role_id.into(), None)
                    .await?;
            } else if interaction
                .user
                .has_role(&ctx.http, guild_id, role_id.into())
                .await?
            {
                ctx.http
                    .remove_member_role(guild_id, interaction.user.id, role_id.into(), None)
                    .await?;
            }
        }

        interaction
            .edit_response(
                &ctx.http,
                EditInteractionResponse::new()
                    .content("Your roles have been updated based on your selection."),
            )
            .await?;

        Ok(Some(()))
    }
}
