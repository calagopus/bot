use crate::sponsors::{Provider, amount, cached_ledger, evaluate};
use poise::CreateReply;
use serenity::all::{
    ButtonStyle, CreateActionRow, CreateButton, CreateComponent, CreateContainer,
    CreateContainerComponent, CreateSection, CreateSectionAccessory, CreateSectionComponent,
    CreateSeparator, CreateTextDisplay, CreateThumbnail, CreateUnfurledMediaItem, MessageFlags,
};

/// View sponsorship information.
#[poise::command(
    slash_command,
    rename = "sponsor",
    subcommands(
        "sponsor_status_command",
        "sponsor_tiers_command",
        "sponsor_link_command",
        "sponsor_unlink_command"
    )
)]
pub async fn sponsor_command(_ctx: crate::BotContext<'_>) -> Result<(), anyhow::Error> {
    Ok(())
}

const HEADING_SPONSORSHIP: &str = "## <:cash:1150889514236137605> Your sponsorship";
const HEADING_LINK: &str = "## <:hammer:1150889684227076227> Link your accounts";
const HEADING_UNLINK: &str = "## <:hammer:1150889684227076227> Unlink your accounts";
pub const HEADING_LINKED: &str = "## <:hammer:1150889684227076227> Account linked";
pub const HEADING_UNLINKED: &str = "## <:hammer:1150889684227076227> Account unlinked";
const HEADING_TIERS: &str = "## <:cash:1150889514236137605> Sponsorship tiers";

fn tier_lines(tiers: &[crate::tiers::Tier], currency: &str) -> String {
    tiers
        .iter()
        .map(|tier| {
            format!(
                "<@&{}> - {}",
                tier.role_id,
                if tier.entry_in_cents <= 0 {
                    "any amount".to_string()
                } else {
                    format!("`{}`+", amount(tier.entry_in_cents, currency))
                }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn contribute_buttons<'a>(env: &crate::env::Env) -> Vec<CreateButton<'a>> {
    Provider::ALL
        .into_iter()
        .filter(|provider| provider.is_configured(env))
        .map(|provider| {
            CreateButton::new_link(provider.contribute_url(env)).label(provider.label())
        })
        .collect()
}

/// List the sponsorship tiers and what they take.
#[poise::command(slash_command, rename = "tiers")]
pub async fn sponsor_tiers_command(ctx: crate::BotContext<'_>) -> Result<(), anyhow::Error> {
    let data = ctx.data();
    let currency = data.env.sponsors_currency.as_str();

    if data.env.donator_tiers.is_empty() && data.env.sponsor_tiers.is_empty() {
        ctx.send(
            CreateReply::default()
                .content("No sponsorship tiers are configured.")
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    }

    let mut components = vec![CreateContainerComponent::TextDisplay(
        CreateTextDisplay::new(HEADING_TIERS),
    )];

    if !data.env.donator_tiers.is_empty() {
        components.push(CreateContainerComponent::Separator(CreateSeparator::new()));
        components.push(CreateContainerComponent::TextDisplay(
            CreateTextDisplay::new(format!(
                "### Donator - by lifetime total
{}",
                tier_lines(&data.env.donator_tiers, currency)
            )),
        ));
    }

    if !data.env.sponsor_tiers.is_empty() {
        components.push(CreateContainerComponent::Separator(CreateSeparator::new()));
        components.push(CreateContainerComponent::TextDisplay(
            CreateTextDisplay::new(format!(
                "### Sponsor - by monthly amount
{}",
                tier_lines(&data.env.sponsor_tiers, currency)
            )),
        ));
    }

    components.push(CreateContainerComponent::TextDisplay(
        CreateTextDisplay::new(
            "-# Contributions on either platform count towards the same totals.",
        ),
    ));

    let buttons = contribute_buttons(&data.env);
    if !buttons.is_empty() {
        components.push(CreateContainerComponent::ActionRow(
            CreateActionRow::Buttons(buttons.into()),
        ));
    }

    ctx.send(
        CreateReply::default()
            .components(&[CreateComponent::Container(CreateContainer::new(components))])
            .flags(MessageFlags::IS_COMPONENTS_V2),
    )
    .await?;

    Ok(())
}

fn linked_logins(accounts: &[crate::models::LinkedAccount], provider: Provider) -> Vec<&str> {
    accounts
        .iter()
        .filter(|account| account.provider == provider.key())
        .filter_map(|account| account.login.as_deref())
        .collect()
}

/// One line per provider saying which accounts, if any, it is linked to.
pub fn link_status_lines(linked: &[crate::models::LinkedAccount]) -> String {
    Provider::ALL
        .into_iter()
        .map(|provider| {
            let logins = linked_logins(linked, provider);

            if logins.is_empty() {
                format!("**{}**: not linked", provider.label())
            } else {
                format!(
                    "**{}**: linked to {}",
                    provider.label(),
                    logins
                        .iter()
                        .map(|login| format!("**{login}**"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The `/sponsor link` message, re-rendered once a link completes. Every button gets a
/// fresh state tied to the same interaction, so the finished flow can report back again.
pub async fn link_components<'a>(
    env: &crate::env::Env,
    linked: &[crate::models::LinkedAccount],
    discord_user_id: u64,
    interaction_token: &str,
) -> Result<Vec<CreateComponent<'a>>, anyhow::Error> {
    let mut buttons = Vec::new();

    for provider in Provider::ALL
        .into_iter()
        .filter(|provider| crate::oauth::is_configured(env, *provider))
    {
        let link_state =
            crate::oauth::create_link_state(provider, discord_user_id, interaction_token).await;
        let url = crate::oauth::authorize_url(env, provider, &link_state)?;

        buttons.push(CreateButton::new_link(url).label(format!("Link {}", provider.label())));
    }

    Ok(vec![CreateComponent::Container(CreateContainer::new(
        vec![
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(HEADING_LINK)),
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(link_status_lines(
                linked,
            ))),
            CreateContainerComponent::Separator(CreateSeparator::new()),
            CreateContainerComponent::ActionRow(CreateActionRow::Buttons(buttons.into())),
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(
                "-# Linking again replaces the existing link for that platform. These links are only valid for you and expire in 10 minutes.",
            )),
        ],
    ))])
}

/// Link a GitHub Sponsors or Open Collective account to your Discord account.
#[poise::command(slash_command, rename = "link")]
pub async fn sponsor_link_command(ctx: crate::BotContext<'_>) -> Result<(), anyhow::Error> {
    let data = ctx.data();

    if !Provider::ALL
        .into_iter()
        .any(|provider| crate::oauth::is_configured(&data.env, provider))
    {
        ctx.send(
            CreateReply::default()
                .content("Account linking is not configured.")
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    }

    let linked = crate::models::find_linked_accounts_by_discord(
        data.database.read(),
        ctx.author().id.get() as i64,
    )
    .await?;

    ctx.send(
        CreateReply::default()
            .components(
                link_components(
                    &data.env,
                    &linked,
                    ctx.author().id.get(),
                    &ctx.interaction.token,
                )
                .await?,
            )
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .ephemeral(true),
    )
    .await?;

    Ok(())
}

/// The `/sponsor unlink` message, re-rendered by the unlink buttons as links go away.
pub fn unlink_components<'a>(linked: &[crate::models::LinkedAccount]) -> Vec<CreateComponent<'a>> {
    let mut lines = Vec::new();
    let mut buttons = Vec::new();

    for provider in Provider::ALL {
        let logins = linked_logins(linked, provider);
        if logins.is_empty() {
            continue;
        }

        lines.push(format!(
            "**{}**: {}",
            provider.label(),
            logins
                .iter()
                .map(|login| format!("**{login}**"))
                .collect::<Vec<_>>()
                .join(", ")
        ));

        buttons.push(
            CreateButton::new(format!(
                "{}{}",
                crate::components::SPONSOR_UNLINK_ID,
                provider.key()
            ))
            .label(format!("Unlink {}", provider.label()))
            .style(ButtonStyle::Danger),
        );
    }

    let mut components = vec![CreateContainerComponent::TextDisplay(
        CreateTextDisplay::new(HEADING_UNLINK),
    )];

    if buttons.is_empty() {
        components.push(CreateContainerComponent::TextDisplay(
            CreateTextDisplay::new(
                "Your Discord account is not linked to any sponsorship account.",
            ),
        ));
    } else {
        components.extend([
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(lines.join("\n"))),
            CreateContainerComponent::Separator(CreateSeparator::new()),
            CreateContainerComponent::ActionRow(CreateActionRow::Buttons(buttons.into())),
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(
                "-# Tier roles are recalculated from whatever stays linked.",
            )),
        ]);
    }

    vec![CreateComponent::Container(CreateContainer::new(components))]
}

/// Unlink a GitHub Sponsors or Open Collective account from your Discord account.
#[poise::command(slash_command, rename = "unlink")]
pub async fn sponsor_unlink_command(ctx: crate::BotContext<'_>) -> Result<(), anyhow::Error> {
    let data = ctx.data();

    let linked = crate::models::find_linked_accounts_by_discord(
        data.database.read(),
        ctx.author().id.get() as i64,
    )
    .await?;

    ctx.send(
        CreateReply::default()
            .components(unlink_components(&linked))
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .ephemeral(true),
    )
    .await?;

    Ok(())
}

/// Show the status of your sponsorship.
#[poise::command(slash_command, rename = "status")]
pub async fn sponsor_status_command(ctx: crate::BotContext<'_>) -> Result<(), anyhow::Error> {
    let data = ctx.data();

    if !crate::sponsors::is_configured(&data.env) {
        ctx.send(
            CreateReply::default()
                .content("Sponsor data is not configured.")
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    }

    ctx.defer_ephemeral().await?;

    let linked = crate::models::find_linked_accounts_by_discord(
        data.database.read(),
        ctx.author().id.get() as i64,
    )
    .await?;

    if linked.is_empty() {
        let linkable = Provider::ALL
            .into_iter()
            .any(|provider| crate::oauth::is_configured(&data.env, provider));

        ctx.send(
            CreateReply::default()
                .content(if !linkable {
                    "Your Discord account is not linked to any sponsorship account."
                } else {
                    "Your Discord account is not linked to a sponsorship account, use `/sponsor link` to link one."
                })
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    }

    let accounts = linked
        .iter()
        .filter_map(|account| Some((account.provider()?, account.provider_id.clone())))
        .collect::<Vec<_>>();

    let ledger = cached_ledger(&data.env).await?;
    let evaluation = evaluate(&ledger);
    let currency = evaluation.currency.as_str();

    let sponsors = accounts
        .iter()
        .filter_map(|(provider, key)| evaluation.sponsor(*provider, key))
        .collect::<Vec<_>>();

    if sponsors.is_empty() {
        let mut components = vec![
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(HEADING_SPONSORSHIP)),
            CreateContainerComponent::TextDisplay(CreateTextDisplay::new(format!(
                "No sponsorship is recorded for {}.",
                linked
                    .iter()
                    .filter_map(|account| account.login.as_deref())
                    .map(|login| format!("**{login}**"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))),
        ];

        let buttons = contribute_buttons(&data.env);
        if !buttons.is_empty() {
            components.push(CreateContainerComponent::ActionRow(
                CreateActionRow::Buttons(buttons.into()),
            ));
        }

        ctx.send(
            CreateReply::default()
                .components(&[CreateComponent::Container(CreateContainer::new(components))])
                .flags(MessageFlags::IS_COMPONENTS_V2)
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    }

    let (lifetime_in_cents, monthly_in_cents) = evaluation.totals(&accounts);
    let one_time_in_cents: i64 = sponsors
        .iter()
        .map(|sponsor| sponsor.one_time_in_cents)
        .sum();
    let recurring_in_cents: i64 = sponsors
        .iter()
        .map(|sponsor| sponsor.recurring_in_cents)
        .sum();
    let estimated_months_paid: u32 = sponsors
        .iter()
        .map(|sponsor| sponsor.estimated_months_paid)
        .max()
        .unwrap_or(0);
    let recurring_spells: usize = sponsors
        .iter()
        .map(|sponsor| sponsor.recurring_spells)
        .sum();

    let status = if sponsors.iter().any(|sponsor| sponsor.active) {
        "Recurring sponsor"
    } else if recurring_spells > 0 {
        "Former recurring sponsor"
    } else {
        "One-time sponsor"
    };

    let roles = crate::tiers::earned_role_ids(&data.env, &evaluation, &accounts);

    let avatar_url = sponsors
        .iter()
        .find_map(|sponsor| sponsor.account.as_ref())
        .map(|account| account.avatar_url.clone())
        .unwrap_or_else(|| sponsors[0].provider.fallback_avatar_url(&data.env));

    let mut components = vec![
        CreateContainerComponent::Section(CreateSection::new(
            vec![
                CreateSectionComponent::TextDisplay(CreateTextDisplay::new(HEADING_SPONSORSHIP)),
                CreateSectionComponent::TextDisplay(CreateTextDisplay::new(format!(
                    "**Status**: {status}
**Monthly**: `{}`",
                    amount(monthly_in_cents, currency)
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
            amount(one_time_in_cents, currency),
            amount(recurring_in_cents, currency),
            amount(lifetime_in_cents, currency),
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

    if sponsors.len() > 1 {
        components.push(CreateContainerComponent::Separator(CreateSeparator::new()));
        components.push(CreateContainerComponent::TextDisplay(
            CreateTextDisplay::new(
                sponsors
                    .iter()
                    .map(|sponsor| {
                        format!(
                            "**{}**: `{}` monthly, `{}` lifetime",
                            sponsor.provider.label(),
                            amount(sponsor.monthly_in_cents, currency),
                            amount(sponsor.lifetime_in_cents, currency)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        ));
    }

    if recurring_spells > 0 {
        components.push(CreateContainerComponent::TextDisplay(
            CreateTextDisplay::new(format!(
                "-# Sponsoring for {} month{}",
                estimated_months_paid,
                if estimated_months_paid == 1 { "" } else { "s" }
            )),
        ));
    }

    let buttons = contribute_buttons(&data.env);
    if !buttons.is_empty() {
        components.push(CreateContainerComponent::ActionRow(
            CreateActionRow::Buttons(buttons.into()),
        ));
    }

    ctx.send(
        CreateReply::default()
            .components(&[CreateComponent::Container(CreateContainer::new(components))])
            .flags(MessageFlags::IS_COMPONENTS_V2)
            .ephemeral(true),
    )
    .await?;

    Ok(())
}
