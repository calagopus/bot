use crate::sponsors::{cached_ledger, dollars, evaluate, sponsors_login};
use poise::CreateReply;
use serenity::all::{
    CreateActionRow, CreateButton, CreateComponent, CreateContainer, CreateContainerComponent,
    CreateSection, CreateSectionAccessory, CreateSectionComponent, CreateSeparator,
    CreateTextDisplay, CreateThumbnail, CreateUnfurledMediaItem, MessageFlags,
};

/// View sponsorship information.
#[poise::command(
    slash_command,
    rename = "sponsor",
    subcommands("sponsor_status_command", "sponsor_tiers_command")
)]
pub async fn sponsor_command(_ctx: crate::BotContext<'_>) -> Result<(), anyhow::Error> {
    Ok(())
}

const HEADING_SPONSORSHIP: &str = "## <:cash:1150889514236137605> Your sponsorship";
const HEADING_TIERS: &str = "## <:cash:1150889514236137605> Sponsorship tiers";

fn tier_lines(tiers: &[crate::tiers::Tier]) -> String {
    tiers
        .iter()
        .map(|tier| {
            format!(
                "<@&{}> - {}",
                tier.role_id,
                if tier.entry_in_cents <= 0 {
                    "any amount".to_string()
                } else {
                    format!("`{}`+", dollars(tier.entry_in_cents))
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// List the sponsorship tiers and what they take.
#[poise::command(slash_command, rename = "tiers")]
pub async fn sponsor_tiers_command(ctx: crate::BotContext<'_>) -> Result<(), anyhow::Error> {
    let data = ctx.data();

    if data.env.donator_tiers.is_empty() && data.env.sponsor_tiers.is_empty() {
        ctx.send(
            CreateReply::default()
                .content("No sponsorship tiers are configured.")
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    }

    let sponsors_url = format!("https://github.com/sponsors/{}", sponsors_login(&data.env));

    let mut components = vec![CreateContainerComponent::TextDisplay(
        CreateTextDisplay::new(HEADING_TIERS),
    )];

    if !data.env.donator_tiers.is_empty() {
        components.push(CreateContainerComponent::Separator(CreateSeparator::new()));
        components.push(CreateContainerComponent::TextDisplay(
            CreateTextDisplay::new(format!(
                "### Donator - by lifetime total
{}",
                tier_lines(&data.env.donator_tiers)
            )),
        ));
    }

    if !data.env.sponsor_tiers.is_empty() {
        components.push(CreateContainerComponent::Separator(CreateSeparator::new()));
        components.push(CreateContainerComponent::TextDisplay(
            CreateTextDisplay::new(format!(
                "### Sponsor - by monthly amount
{}",
                tier_lines(&data.env.sponsor_tiers)
            )),
        ));
    }

    components.push(CreateContainerComponent::ActionRow(
        CreateActionRow::Buttons(
            vec![CreateButton::new_link(&sponsors_url).label("GitHub Sponsors")].into(),
        ),
    ));

    ctx.send(
        CreateReply::default()
            .components(&[CreateComponent::Container(CreateContainer::new(components))])
            .flags(MessageFlags::IS_COMPONENTS_V2),
    )
    .await?;

    Ok(())
}

/// Show the status of your sponsorship.
#[poise::command(slash_command, rename = "status")]
pub async fn sponsor_status_command(ctx: crate::BotContext<'_>) -> Result<(), anyhow::Error> {
    let data = ctx.data();

    if data.env.github_token.is_none() {
        ctx.send(
            CreateReply::default()
                .content("Sponsor data is not configured.")
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    }

    ctx.defer_ephemeral().await?;

    let Some(user) = crate::models::find_github_user_by_discord(
        data.database.read(),
        ctx.author().id.get() as i64,
    )
    .await?
    else {
        ctx.send(
            CreateReply::default()
                .content(
                    "Your Discord account is not linked to a GitHub account, use `/github link` to link it.",
                )
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    };

    let ledger = cached_ledger(&data.env).await?;
    let evaluation = evaluate(&ledger, chrono::Utc::now());

    let sponsors_url = format!("https://github.com/sponsors/{}", sponsors_login(&data.env));

    let Some(sponsor) = evaluation
        .sponsors
        .iter()
        .find(|sponsor| sponsor.github_id == Some(user.id))
    else {
        ctx.send(
            CreateReply::default()
                .components(&[CreateComponent::Container(CreateContainer::new(&[
                    CreateContainerComponent::TextDisplay(CreateTextDisplay::new(
                        HEADING_SPONSORSHIP,
                    )),
                    CreateContainerComponent::TextDisplay(CreateTextDisplay::new(format!(
                        "No sponsorship is recorded for **{}**.",
                        user.login.as_deref().unwrap_or("your GitHub account")
                    ))),
                    CreateContainerComponent::ActionRow(CreateActionRow::Buttons(
                        (&[CreateButton::new_link(&sponsors_url).label("GitHub Sponsors")]).into(),
                    )),
                ]))])
                .flags(MessageFlags::IS_COMPONENTS_V2)
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    };

    let status = if sponsor.active {
        "Monthly sponsor"
    } else if sponsor.recurring_spells > 0 {
        "Former monthly sponsor"
    } else {
        "One-time sponsor"
    };

    let roles = crate::tiers::earned_role_ids(&data.env, &evaluation, user.id);

    let avatar_url = match user.login.as_deref() {
        Some(login) => format!("https://github.com/{login}.png"),
        None => format!("https://github.com/{}.png", sponsors_login(&data.env)),
    };

    let mut components = vec![
        CreateContainerComponent::Section(CreateSection::new(
            vec![
                CreateSectionComponent::TextDisplay(CreateTextDisplay::new(HEADING_SPONSORSHIP)),
                CreateSectionComponent::TextDisplay(CreateTextDisplay::new(format!(
                    "**Status**: {status}
**Monthly**: `{}`",
                    dollars(sponsor.monthly_in_cents)
                ))),
            ],
            CreateSectionAccessory::Thumbnail(CreateThumbnail::new(CreateUnfurledMediaItem::new(
                avatar_url,
            ))),
        )),
        CreateContainerComponent::Separator(CreateSeparator::new()),
        CreateContainerComponent::TextDisplay(CreateTextDisplay::new(format!(
            "**One-time**: `{}`
**Recurring**: `{}`
**Lifetime**: `{}`{}",
            dollars(sponsor.one_time_in_cents),
            dollars(sponsor.recurring_in_cents),
            dollars(sponsor.lifetime_in_cents),
            if roles.is_empty() {
                String::new()
            } else {
                format!(
                    "\n**Roles**: {}",
                    roles
                        .iter()
                        .map(|role_id| format!("<@&{role_id}>"))
                        .collect::<Vec<_>>()
                        .join(" ")
                )
            }
        ))),
    ];

    if sponsor.recurring_spells > 0 {
        components.push(CreateContainerComponent::TextDisplay(
            CreateTextDisplay::new(format!(
                "-# Sponsoring for {} month{}",
                sponsor.estimated_months_paid,
                if sponsor.estimated_months_paid == 1 {
                    ""
                } else {
                    "s"
                }
            )),
        ));
    }

    components.push(CreateContainerComponent::ActionRow(
        CreateActionRow::Buttons(
            vec![CreateButton::new_link(&sponsors_url).label("GitHub Sponsors")].into(),
        ),
    ));

    ctx.send(
        CreateReply::default()
            .components(&[CreateComponent::Container(CreateContainer::new(components))])
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .ephemeral(true),
    )
    .await?;

    Ok(())
}
