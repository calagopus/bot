mod text_message;

/// Manage administrative commands.
#[poise::command(
    slash_command,
    rename = "admin",
    subcommands("text_message::admin_text_message_command"),
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn admin_command(_ctx: crate::BotContext<'_>) -> Result<(), anyhow::Error> {
    Ok(())
}
