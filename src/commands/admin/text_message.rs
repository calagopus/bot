use crate::modals::text_message_modify::TextMessageModifyModal;
use poise::{CreateReply, Modal};
use serenity::all::GuildChannel;

/// Manage text messages sent by the bot.
#[poise::command(
    slash_command,
    rename = "text-message",
    subcommands(
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

/// Add a new text message to be sent by the bot.
#[poise::command(slash_command, rename = "add")]
pub async fn admin_text_message_add_command(
    ctx: crate::BotContext<'_>,
    #[description = "The channel to send the message to"] channel: GuildChannel,
) -> Result<(), anyhow::Error> {
    let Some(data) = TextMessageModifyModal::execute(ctx).await? else {
        return Ok(());
    };

    let mut text_message: crate::models::TextMessage = sqlx::query_as(
        "INSERT INTO text_messages (channel_id, title, content) VALUES (?, ?, ?) RETURNING *",
    )
    .bind(channel.id.get() as i64)
    .bind(&data.title)
    .bind(&data.content)
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

    let Some(data) = TextMessageModifyModal::execute_with_defaults(
        ctx,
        TextMessageModifyModal {
            title: text_message.title,
            content: text_message.content,
        },
    )
    .await?
    else {
        return Ok(());
    };

    text_message.title = data.title;
    text_message.content = data.content;

    sqlx::query("UPDATE text_messages SET title = ?, content = ? WHERE id = ?")
        .bind(&text_message.title)
        .bind(&text_message.content)
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
