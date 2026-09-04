use poise::CreateReply;
use serenity::all::{
    CreateActionRow, CreateButton, CreateComponent, CreateContainer, CreateContainerComponent,
    CreateSeparator, CreateTextDisplay, MessageFlags,
};

const HEADING_LINK: &str = "## <:hammer:1150889684227076227> Link your GitHub account";

/// Manage the link between your Discord and GitHub accounts.
#[poise::command(
    slash_command,
    rename = "github",
    subcommands("github_link_command", "github_unlink_command")
)]
pub async fn github_command(_ctx: crate::BotContext<'_>) -> Result<(), anyhow::Error> {
    Ok(())
}

/// Link your GitHub account to your Discord account.
#[poise::command(slash_command, rename = "link")]
pub async fn github_link_command(ctx: crate::BotContext<'_>) -> Result<(), anyhow::Error> {
    let data = ctx.data();

    if !crate::oauth::is_configured(&data.env) {
        ctx.send(
            CreateReply::default()
                .content("GitHub account linking is not configured.")
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    }

    let existing = crate::models::find_github_user_by_discord(
        data.database.read(),
        ctx.author().id.get() as i64,
    )
    .await?;

    let description = match existing.as_ref().and_then(|user| user.login.as_deref()) {
        Some(login) => format!(
            "Your Discord account is linked to [**{login}**](https://github.com/{login}).
Authorizing again will replace that link."
        ),
        None => {
            "Authorize the GitHub app below to link your GitHub account to your Discord account."
                .to_string()
        }
    };

    let link_state = crate::oauth::create_link_state(ctx.author().id.get()).await;
    let url = crate::oauth::authorize_url(&data.env, &link_state)?;

    ctx.send(
        CreateReply::default()
            .components(&[CreateComponent::Container(CreateContainer::new(&[
                CreateContainerComponent::TextDisplay(CreateTextDisplay::new(HEADING_LINK)),
                CreateContainerComponent::TextDisplay(CreateTextDisplay::new(description)),
                CreateContainerComponent::Separator(CreateSeparator::new()),
                CreateContainerComponent::ActionRow(CreateActionRow::Buttons(
                    (&[CreateButton::new_link(url).label("Link GitHub")]).into(),
                )),
                CreateContainerComponent::TextDisplay(CreateTextDisplay::new(
                    "-# This link is only valid for you and expires in 10 minutes.",
                )),
            ]))])
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .ephemeral(true),
    )
    .await?;

    Ok(())
}

/// Unlink your GitHub account from your Discord account.
#[poise::command(slash_command, rename = "unlink")]
pub async fn github_unlink_command(ctx: crate::BotContext<'_>) -> Result<(), anyhow::Error> {
    let data = ctx.data();

    let Some(user) =
        crate::models::unlink_discord_user(data.database.write(), ctx.author().id.get() as i64)
            .await?
    else {
        ctx.send(
            CreateReply::default()
                .content("Your Discord account is not linked to a GitHub account.")
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    };

    if !crate::tiers::managed_role_ids(&data.env).is_empty() {
        match crate::utils::guild_id(&data).await {
            Ok(guild_id) => {
                crate::tiers::apply_roles(&data, guild_id, ctx.author().id, &[]).await?;
            }
            Err(err) => tracing::error!(
                "failed to resolve guild to remove sponsorship tier roles: {:?}",
                err
            ),
        }
    }

    ctx.send(
        CreateReply::default()
            .content(format!(
                "Unlinked your GitHub account ({}).",
                user.login.as_deref().unwrap_or("unknown")
            ))
            .ephemeral(true),
    )
    .await?;

    Ok(())
}
