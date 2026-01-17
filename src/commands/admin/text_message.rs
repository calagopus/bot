use crate::modals::{text_message::TextMessageModal, text_message_modify::TextMessageModifyModal};
use indexmap::IndexMap;
use poise::{CreateReply, Modal};
use serenity::all::GuildChannel;

/// Manage text messages sent by the bot.
#[poise::command(
    slash_command,
    rename = "text-message",
    subcommands(
        "admin_text_message_send_once_command",
        "admin_text_message_add_command",
        "admin_text_message_update_command",
        "admin_text_message_sync_command",
        "admin_text_message_recreate_command",
        "admin_text_message_delete_command"
    )
)]
pub async fn admin_text_message_command(_ctx: crate::BotContext<'_>) -> Result<(), anyhow::Error> {
    Ok(())
}

/// Sends a new text message using the bot.
#[poise::command(slash_command, rename = "send-once")]
pub async fn admin_text_message_send_once_command(
    ctx: crate::BotContext<'_>,
    #[description = "The channel to send the message to"] channel: GuildChannel,
) -> Result<(), anyhow::Error> {
    let Some(data) = TextMessageModal::execute(ctx).await? else {
        return Ok(());
    };

    let message = channel
        .send_message(
            ctx.http(),
            serenity::all::CreateMessage::new()
                .components(&[serenity::all::CreateComponent::Container(
                    serenity::all::CreateContainer::new(&[
                        serenity::all::CreateContainerComponent::TextDisplay(
                            serenity::all::CreateTextDisplay::new(format!("## {}", data.title)),
                        ),
                        serenity::all::CreateContainerComponent::TextDisplay(
                            serenity::all::CreateTextDisplay::new(data.content),
                        ),
                    ]),
                )])
                .flags(serenity::all::MessageFlags::IS_COMPONENTS_V2),
        )
        .await?;

    ctx.send(
        CreateReply::default()
            .content(format!("Text message sent. ({})", message.link()))
            .ephemeral(true),
    )
    .await?;

    Ok(())
}

/// Add a new text message to be sent by the bot.
#[poise::command(slash_command, rename = "add")]
pub async fn admin_text_message_add_command(
    ctx: crate::BotContext<'_>,
    #[description = "The channel to send the message to"] channel: GuildChannel,
) -> Result<(), anyhow::Error> {
    let Some(data) = TextMessageModifyModal::execute(ctx).await? else {
        return Ok(());
    };

    let mut roles = IndexMap::new();
    for role in data.roles.unwrap_or_default() {
        roles.insert(role.id.get(), role.name.to_string());
    }

    let mut text_message: crate::models::TextMessage = sqlx::query_as(
        "INSERT INTO text_messages (channel_id, title, content, roles) VALUES (?, ?, ?, ?) RETURNING *",
    )
    .bind(channel.id.get() as i64)
    .bind(&data.title)
    .bind(&data.content)
    .bind(serde_json::to_string(&roles)?)
    .fetch_one(ctx.data().database.write())
    .await?;

    let message = text_message
        .send_or_update(&ctx.serenity_context().http, &ctx.data().database)
        .await?;

    ctx.send(
        CreateReply::default()
            .content(format!("Text message added. ({})", message.link()))
            .ephemeral(true),
    )
    .await?;

    Ok(())
}

/// Update an existing text message.
#[poise::command(slash_command, rename = "update")]
pub async fn admin_text_message_update_command(
    ctx: crate::BotContext<'_>,
    #[description = "The text message to update"]
    #[autocomplete = "crate::models::autocomplete_text_message_id"]
    text_message: u64,
) -> Result<(), anyhow::Error> {
    let Some(mut text_message): Option<crate::models::TextMessage> =
        sqlx::query_as("SELECT * FROM text_messages WHERE id = ?")
            .bind(text_message as i64)
            .fetch_optional(ctx.data().database.write())
            .await?
    else {
        ctx.send(
            CreateReply::default()
                .content("Text message not found.")
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    };

    let guild_id = match ctx.guild_id() {
        Some(guild) => guild,
        None => {
            ctx.send(
                CreateReply::default()
                    .content("This command can only be used in a guild.")
                    .ephemeral(true),
            )
            .await?;
            return Ok(());
        }
    };

    let mut roles = Vec::new();
    for role_id in text_message.roles.keys() {
        roles.push(
            ctx.http()
                .get_guild_role(guild_id, (*role_id).into())
                .await?,
        );
    }

    let Some(data) = TextMessageModifyModal::execute_with_defaults(
        ctx,
        TextMessageModifyModal {
            title: text_message.title,
            content: text_message.content,
            roles: Some(roles),
        },
    )
    .await?
    else {
        return Ok(());
    };

    ctx.defer_ephemeral().await?;

    text_message.title = data.title;
    text_message.content = data.content;
    text_message.roles = data
        .roles
        .unwrap_or_default()
        .into_iter()
        .map(|role| (role.id.get(), role.name.to_string()))
        .collect();

    sqlx::query("UPDATE text_messages SET title = ?, content = ?, roles = ? WHERE id = ?")
        .bind(&text_message.title)
        .bind(&text_message.content)
        .bind(serde_json::to_string(&text_message.roles)?)
        .bind(text_message.id)
        .execute(ctx.data().database.write())
        .await?;

    text_message
        .send_or_update(&ctx.serenity_context().http, &ctx.data().database)
        .await?;

    ctx.send(
        CreateReply::default()
            .content("Text message updated.")
            .ephemeral(true),
    )
    .await?;

    Ok(())
}

/// Sync an existing text message.
#[poise::command(slash_command, rename = "sync")]
pub async fn admin_text_message_sync_command(
    ctx: crate::BotContext<'_>,
    #[description = "The text message to sync"]
    #[autocomplete = "crate::models::autocomplete_text_message_id"]
    text_message: u64,
) -> Result<(), anyhow::Error> {
    let Some(mut text_message): Option<crate::models::TextMessage> =
        sqlx::query_as("SELECT * FROM text_messages WHERE id = ?")
            .bind(text_message as i64)
            .fetch_optional(ctx.data().database.write())
            .await?
    else {
        ctx.send(
            CreateReply::default()
                .content("Text message not found.")
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    };

    let message = text_message
        .send_or_update(&ctx.serenity_context().http, &ctx.data().database)
        .await?;

    ctx.send(
        CreateReply::default()
            .content(format!("Text message synced. ({})", message.link()))
            .ephemeral(true),
    )
    .await?;

    Ok(())
}

/// Sync an existing text message.
#[poise::command(slash_command, rename = "recreate")]
pub async fn admin_text_message_recreate_command(
    ctx: crate::BotContext<'_>,
    #[description = "The text message to recreate"]
    #[autocomplete = "crate::models::autocomplete_text_message_id"]
    text_message: u64,
) -> Result<(), anyhow::Error> {
    let Some(mut text_message): Option<crate::models::TextMessage> =
        sqlx::query_as("SELECT * FROM text_messages WHERE id = ?")
            .bind(text_message as i64)
            .fetch_optional(ctx.data().database.write())
            .await?
    else {
        ctx.send(
            CreateReply::default()
                .content("Text message not found.")
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    };

    if let (_, Some(message)) = text_message.get_message(ctx.http()).await? {
        message.delete(ctx.http(), None).await?;
        text_message.message_id = None;
    }

    let message = text_message
        .send_or_update(&ctx.serenity_context().http, &ctx.data().database)
        .await?;

    ctx.send(
        CreateReply::default()
            .content(format!("Text message recreated. ({})", message.link()))
            .ephemeral(true),
    )
    .await?;

    Ok(())
}

/// Delete an existing text message.
#[poise::command(slash_command, rename = "delete")]
pub async fn admin_text_message_delete_command(
    ctx: crate::BotContext<'_>,
    #[description = "The text message to delete"]
    #[autocomplete = "crate::models::autocomplete_text_message_id"]
    text_message: u64,
) -> Result<(), anyhow::Error> {
    let Some(text_message): Option<crate::models::TextMessage> =
        sqlx::query_as("SELECT * FROM text_messages WHERE id = ?")
            .bind(text_message as i64)
            .fetch_optional(ctx.data().database.write())
            .await?
    else {
        ctx.send(
            CreateReply::default()
                .content("Text message not found.")
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    };

    if let (_, Some(message)) = text_message.get_message(ctx.http()).await? {
        message.delete(ctx.http(), None).await?;
    }

    sqlx::query("DELETE FROM text_messages WHERE id = ?")
        .bind(text_message.id)
        .execute(ctx.data().database.write())
        .await?;

    ctx.send(
        CreateReply::default()
            .content("Text message deleted.")
            .ephemeral(true),
    )
    .await?;

    Ok(())
}
