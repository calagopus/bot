use poise::CreateReply;
use serenity::all::{
    CreateActionRow, CreateButton, CreateComponent, CreateContainer, CreateContainerComponent,
    CreateSection, CreateSectionAccessory, CreateSectionComponent, CreateSeparator,
    CreateTextDisplay, CreateThumbnail, CreateUnfurledMediaItem, MessageFlags,
};

/// Check the bot's status.
#[poise::command(slash_command, rename = "status")]
pub async fn status_command(ctx: crate::BotContext<'_>) -> Result<(), anyhow::Error> {
    let ping = ctx.ping().await.unwrap_or_default().as_millis();

    let uptime = ctx.data().start_time.elapsed();
    let uptime = if uptime.as_secs() < 60 {
        format!("{}s", uptime.as_secs())
    } else if uptime.as_secs() < 3600 {
        format!("{}m {}s", uptime.as_secs() / 60, uptime.as_secs() % 60)
    } else if uptime.as_secs() < 86400 {
        format!(
            "{}h {}m",
            uptime.as_secs() / 3600,
            (uptime.as_secs() % 3600) / 60
        )
    } else {
        format!(
            "{}d {}h {}m",
            uptime.as_secs() / 86400,
            (uptime.as_secs() % 86400) / 3600,
            (uptime.as_secs() % 3600) / 60
        )
    };

    ctx.send(
        CreateReply::default()
            .components(&[CreateComponent::Container(CreateContainer::new(&[
                CreateContainerComponent::Section(CreateSection::new(
                    &[
                        CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                            "## Bot Status",
                        )),
                        CreateSectionComponent::TextDisplay(CreateTextDisplay::new(format!(
                            "**Uptime**: {uptime}
**Ping**: {ping} ms"
                        ))),
                    ],
                    CreateSectionAccessory::Thumbnail(CreateThumbnail::new(
                        CreateUnfurledMediaItem::new("https://demo.calagopus.com/icon.png"),
                    )),
                )),
                CreateContainerComponent::Separator(CreateSeparator::new(true)),
                CreateContainerComponent::ActionRow(CreateActionRow::Buttons(
                    (&[
                        CreateButton::new_link("https://calagopus.com").label("Website"),
                        CreateButton::new_link("https://github.com/calagopus").label("GitHub"),
                    ])
                        .into(),
                )),
                CreateContainerComponent::TextDisplay(CreateTextDisplay::new(format!(
                    "-# {}",
                    ctx.data().version
                ))),
            ]))])
            .flags(MessageFlags::IS_COMPONENTS_V2),
    )
    .await?;

    Ok(())
}
