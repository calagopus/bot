use super::{Evaluation, Ledger, amount};
use indexmap::IndexMap;
use serenity::all::{
    ButtonStyle, CreateActionRow, CreateButton, CreateComponent, CreateEmbed,
    CreateInteractionResponse, CreateInteractionResponseMessage, CreateMessage, GenericChannelId,
};

pub const PERIOD_DAYS: i64 = 31;

/// What a sponsor gets for their money. Amounts are in [`crate::env::Env::sponsors_currency`]
/// cents, the same currency every other total is normalised to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionTier {
    Name,
    Description,
    Logo,
    Link,
}

impl SectionTier {
    pub const ALL: [Self; 4] = [Self::Name, Self::Description, Self::Logo, Self::Link];

    #[inline]
    pub fn cents(self) -> i64 {
        match self {
            Self::Name => 1000,
            Self::Description => 2000,
            Self::Logo => 5000,
            Self::Link => 10000,
        }
    }

    #[inline]
    pub fn from_cents(cents: i64) -> Option<Self> {
        Self::ALL.into_iter().find(|tier| tier.cents() == cents)
    }

    #[inline]
    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Description => "Name and description",
            Self::Logo => "Name, description and logo",
            Self::Link => "Name, description, logo and link",
        }
    }

    #[inline]
    pub fn has_description(self) -> bool {
        self != Self::Name
    }

    #[inline]
    pub fn has_logo(self) -> bool {
        matches!(self, Self::Logo | Self::Link)
    }

    #[inline]
    pub fn has_link(self) -> bool {
        self == Self::Link
    }
}

#[inline]
pub fn sections_configured(env: &crate::env::Env) -> bool {
    env.sponsor_sections_channel_id.is_some()
        && env.s3_endpoint.is_some()
        && env.s3_bucket.is_some()
        && env.s3_access_key.is_some()
        && env.s3_secret_key.is_some()
        && env.s3_public_url.is_some()
}

pub fn refund_cents(
    section: &crate::models::SponsorSection,
    now: chrono::DateTime<chrono::Utc>,
) -> i64 {
    if !section.active {
        return 0;
    }

    let remaining_days = (section.period_end - now).num_days().clamp(0, PERIOD_DAYS);

    section.tier_cents * remaining_days / PERIOD_DAYS
}

pub fn logo_url(state: &crate::routes::State, section_logo_path: Option<&str>) -> Option<String> {
    let (storage, path) = (state.storage.as_ref()?, section_logo_path?);

    Some(storage.url(path))
}

pub async fn remove_logo(state: &crate::routes::State, path: Option<&str>) {
    let (Some(storage), Some(path)) = (state.storage.as_ref(), path) else {
        return;
    };

    if let Err(err) = storage.remove(path).await {
        tracing::error!("failed to remove sponsor section logo {}: {:?}", path, err);
    }
}

async fn moderator_channel(state: &crate::routes::State) -> Option<GenericChannelId> {
    state
        .env
        .sponsor_sections_channel_id
        .map(GenericChannelId::new)
}

pub async fn alert(
    state: &crate::routes::State,
    title: &str,
    description: String,
) -> Result<(), anyhow::Error> {
    let Some(channel) = moderator_channel(state).await else {
        return Ok(());
    };

    channel
        .send_message(
            &*state.bot.read().await,
            CreateMessage::new().embed(
                CreateEmbed::new()
                    .title(title.to_string())
                    .description(description)
                    .colour(0xffa500)
                    .timestamp(chrono::Utc::now()),
            ),
        )
        .await?;

    Ok(())
}

fn request_embed<'a>(
    state: &crate::routes::State,
    request: &crate::models::SponsorSectionRequest,
    current: Option<&crate::models::SponsorSection>,
    available_cents: i64,
    decision: Option<&str>,
) -> CreateEmbed<'a> {
    let currency = state.env.sponsors_currency.as_str();

    let mut embed = CreateEmbed::new()
        .title(match decision {
            Some(decision) => format!("Sponsor section {decision}"),
            None => "Sponsor section request".to_string(),
        })
        .description(format!("<@{}>", request.discord_user_id))
        .colour(match decision {
            Some("approved") => 0x57f287,
            Some(_) => 0xed4245,
            None => 0x5865f2,
        })
        .field(
            "Tier",
            format!(
                "{} - `{}` every {PERIOD_DAYS} days",
                SectionTier::from_cents(request.tier_cents)
                    .map(SectionTier::label)
                    .unwrap_or("Unknown"),
                amount(request.tier_cents, currency)
            ),
            false,
        )
        .field("Name", request.name.clone(), false);

    if let Some(description) = &request.description {
        embed = embed.field("Description", description.clone(), false);
    }

    if let Some(link) = &request.link {
        embed = embed.field("Link", link.clone(), false);
    }

    if let Some(url) = logo_url(state, request.logo_path.as_deref()) {
        embed = embed.thumbnail(url, None);
    }

    embed = embed.field(
        "Funds",
        format!("`{}` left to spend", amount(available_cents, currency)),
        true,
    );

    embed = embed.field(
        "Replaces",
        match current {
            Some(current) if current.active => format!(
                "`{}` section running until <t:{}:D>",
                amount(current.tier_cents, currency),
                current.period_end.timestamp()
            ),
            _ => "Nothing, this is a new section".to_string(),
        },
        true,
    );

    embed
        .footer(serenity::all::CreateEmbedFooter::new(format!(
            "Request #{}",
            request.id
        )))
        .timestamp(chrono::Utc::now())
}

fn decision_buttons<'a>(request_id: i64) -> CreateComponent<'a> {
    CreateComponent::ActionRow(CreateActionRow::Buttons(
        vec![
            CreateButton::new(format!(
                "{}{request_id}",
                crate::components::SPONSOR_SECTION_APPROVE_ID
            ))
            .label("Approve")
            .style(ButtonStyle::Success),
            CreateButton::new(format!(
                "{}{request_id}",
                crate::components::SPONSOR_SECTION_DENY_ID
            ))
            .label("Deny")
            .style(ButtonStyle::Danger),
        ]
        .into(),
    ))
}

pub fn revoke_button<'a>(discord_user_id: i64) -> CreateComponent<'a> {
    CreateComponent::ActionRow(CreateActionRow::Buttons(
        vec![
            CreateButton::new(format!(
                "{}{discord_user_id}",
                crate::components::SPONSOR_SECTION_REVOKE_ID
            ))
            .label("Revoke section")
            .style(ButtonStyle::Danger),
        ]
        .into(),
    ))
}

pub fn reinstate_button<'a>(discord_user_id: i64) -> CreateComponent<'a> {
    CreateComponent::ActionRow(CreateActionRow::Buttons(
        vec![
            CreateButton::new(format!(
                "{}{discord_user_id}",
                crate::components::SPONSOR_SECTION_REINSTATE_ID
            ))
            .label("Reinstate section")
            .style(ButtonStyle::Success),
        ]
        .into(),
    ))
}

pub async fn post_request(
    state: &crate::routes::State,
    request: &crate::models::SponsorSectionRequest,
) -> Result<(), anyhow::Error> {
    let Some(channel) = moderator_channel(state).await else {
        return Ok(());
    };

    let current =
        crate::models::find_sponsor_section(state.database.read(), request.discord_user_id).await?;
    let available_cents =
        crate::models::available_cents(state.database.read(), request.discord_user_id).await?;

    let message = channel
        .send_message(
            &*state.bot.read().await,
            CreateMessage::new()
                .embed(request_embed(
                    state,
                    request,
                    current.as_ref(),
                    available_cents,
                    None,
                ))
                .components(vec![decision_buttons(request.id)]),
        )
        .await?;

    crate::models::set_sponsor_section_request_message(
        state.database.write(),
        request.id,
        message.id.get() as i64,
    )
    .await?;

    Ok(())
}

pub async fn close_request(
    state: &crate::routes::State,
    request: &crate::models::SponsorSectionRequest,
    replaced: Option<&crate::models::SponsorSection>,
    interaction: &serenity::all::ComponentInteraction,
    approved: bool,
) -> Result<(), anyhow::Error> {
    let available_cents =
        crate::models::available_cents(state.database.read(), request.discord_user_id).await?;

    let embed = request_embed(
        state,
        request,
        replaced,
        available_cents,
        Some(if approved { "approved" } else { "denied" }),
    )
    .field(
        "Decision",
        format!(
            "{} by <@{}>",
            if approved { "Approved" } else { "Denied" },
            interaction.user.id.get()
        ),
        false,
    );

    let components = if approved {
        vec![revoke_button(request.discord_user_id)]
    } else {
        Vec::new()
    };

    interaction
        .create_response(
            &state.bot.read().await.clone(),
            CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new()
                    .embed(embed)
                    .components(components),
            ),
        )
        .await?;

    Ok(())
}

async fn seed_marks(
    env: &crate::env::Env,
    credited_cents: i64,
    native_lifetime_cents: &IndexMap<String, i64>,
) -> Result<IndexMap<String, i64>, anyhow::Error> {
    let mut marks = IndexMap::new();

    for (currency, cents) in native_lifetime_cents {
        let credited =
            super::convert(env, credited_cents, &env.sponsors_currency, currency).await?;

        marks.insert(currency.clone(), credited.min(*cents));
    }

    Ok(marks)
}

async fn sync_balances(
    state: &crate::routes::State,
    ledger: &Ledger,
    evaluation: &Evaluation,
) -> Result<(), anyhow::Error> {
    let accounts = crate::models::linked_accounts(state.database.read()).await?;

    for account in accounts {
        let (Some(discord_user_id), Some(provider)) = (account.discord_user_id, account.provider())
        else {
            continue;
        };

        if !ledger.is_complete_for(&[(provider, account.provider_id.clone())]) {
            continue;
        }

        let native_lifetime_cents = evaluation
            .sponsor(provider, &account.provider_id)
            .map(|sponsor| sponsor.native_lifetime_in_cents.clone())
            .unwrap_or_default();

        let mut marks = match &account.native_lifetime_cents {
            Some(marks) => marks.clone(),
            None => seed_marks(&state.env, account.lifetime_cents, &native_lifetime_cents).await?,
        };

        let mut gained = 0;
        for (currency, cents) in &native_lifetime_cents {
            let credited = marks.get(currency).copied().unwrap_or(0);
            if *cents <= credited {
                continue;
            }

            gained += super::convert(
                &state.env,
                cents - credited,
                currency,
                &state.env.sponsors_currency,
            )
            .await?;
            marks.insert(currency.clone(), *cents);
        }

        if gained > 0 {
            crate::models::add_available_cents(state.database.write(), discord_user_id, gained)
                .await?;
        }

        if gained > 0 || account.native_lifetime_cents.as_ref() != Some(&marks) {
            crate::models::credit_linked_account(
                state.database.write(),
                provider,
                &account.provider_id,
                gained,
                &marks,
            )
            .await?;
        }

        if gained > 0 {
            tracing::info!(
                "credited {} to {} for {} account {}",
                amount(gained, &state.env.sponsors_currency),
                discord_user_id,
                provider.key(),
                account.provider_id
            );
        }

        let mut refunded: IndexMap<String, i64> = IndexMap::new();
        for (currency, cents) in &marks {
            let reported = native_lifetime_cents.get(currency).copied().unwrap_or(0);

            if reported < *cents {
                refunded.insert(currency.clone(), cents - reported);
            }
        }

        let announced = |currency: &String| {
            account
                .native_refunded_cents
                .get(currency)
                .copied()
                .unwrap_or(0)
        };
        let watermarks: IndexMap<String, i64> = refunded
            .iter()
            .map(|(currency, cents)| (currency.clone(), (*cents).max(announced(currency))))
            .collect();

        if watermarks != account.native_refunded_cents {
            crate::models::set_linked_account_refunded(
                state.database.write(),
                provider,
                &account.provider_id,
                &watermarks,
            )
            .await?;
        }

        if !refunded
            .iter()
            .any(|(currency, cents)| *cents > announced(currency))
        {
            continue;
        }

        alert(
            state,
            "Sponsor was refunded",
            format!(
                "<@{}> ({} `{}`) is now {} below what they were credited. Funds already handed out were left alone.",
                discord_user_id,
                provider.label(),
                account.login.as_deref().unwrap_or(&account.provider_id),
                refunded
                    .iter()
                    .map(|(currency, cents)| format!("`{}`", amount(*cents, currency)))
                    .collect::<Vec<_>>()
                    .join(" and ")
            ),
        )
        .await?;
    }

    Ok(())
}

async fn charge_renewals(state: &crate::routes::State) -> Result<(), anyhow::Error> {
    let now = chrono::Utc::now();
    let currency = state.env.sponsors_currency.as_str();

    for section in crate::models::active_sponsor_sections(state.database.read()).await? {
        if section.period_end > now {
            let due_soon = (section.period_end - now).num_days() <= 3;
            let available =
                crate::models::available_cents(state.database.read(), section.discord_user_id)
                    .await?;

            if due_soon && !section.renewal_warned && available < section.tier_cents {
                crate::models::mark_sponsor_section_warned(
                    state.database.write(),
                    section.discord_user_id,
                )
                .await?;

                alert(
                    state,
                    "Sponsor section is about to run out of funds",
                    format!(
                        "<@{}>'s section (**{}**) renews <t:{}:R> for `{}`, but they only have `{}` left to spend.",
                        section.discord_user_id,
                        section.name,
                        section.period_end.timestamp(),
                        amount(section.tier_cents, currency),
                        amount(available, currency)
                    ),
                )
                .await?;
            }

            continue;
        }

        let charged = crate::models::take_available_cents(
            state.database.write(),
            section.discord_user_id,
            section.tier_cents,
        )
        .await?;

        if !charged {
            crate::models::deactivate_sponsor_section(
                state.database.write(),
                section.discord_user_id,
            )
            .await?;

            alert(
                state,
                "Sponsor section ran out of funds",
                format!(
                    "<@{}>'s section (**{}**) needed `{}` to renew and has been taken off the website. Their content is kept and starts again once they have the funds.",
                    section.discord_user_id,
                    section.name,
                    amount(section.tier_cents, currency)
                ),
            )
            .await?;

            continue;
        }

        crate::models::renew_sponsor_section(
            state.database.write(),
            section.discord_user_id,
            section.period_end,
            section.period_end + chrono::Duration::days(PERIOD_DAYS),
        )
        .await?;

        tracing::info!(
            "renewed sponsor section for {} at {}",
            section.discord_user_id,
            amount(section.tier_cents, currency)
        );
    }

    Ok(())
}

pub async fn run_sections(
    state: &crate::routes::State,
    ledger: &Ledger,
    evaluation: &Evaluation,
) -> Result<(), anyhow::Error> {
    sync_balances(state, ledger, evaluation).await?;

    if sections_configured(&state.env) {
        charge_renewals(state).await?;
    }

    Ok(())
}

pub async fn end_section(
    state: &crate::routes::State,
    section: &crate::models::SponsorSection,
) -> Result<i64, anyhow::Error> {
    let refund = refund_cents(section, chrono::Utc::now());

    crate::models::deactivate_sponsor_section(state.database.write(), section.discord_user_id)
        .await?;

    if refund > 0 {
        crate::models::add_available_cents(state.database.write(), section.discord_user_id, refund)
            .await?;
    }

    Ok(refund)
}

pub async fn reinstate_section(
    state: &crate::routes::State,
    section: &crate::models::SponsorSection,
) -> Result<bool, anyhow::Error> {
    let charged = crate::models::take_available_cents(
        state.database.write(),
        section.discord_user_id,
        section.tier_cents,
    )
    .await?;

    if !charged {
        return Ok(false);
    }

    let now = chrono::Utc::now();
    crate::models::renew_sponsor_section(
        state.database.write(),
        section.discord_user_id,
        now,
        now + chrono::Duration::days(PERIOD_DAYS),
    )
    .await?;

    Ok(true)
}

pub async fn start_section(
    state: &crate::routes::State,
    request: &crate::models::SponsorSectionRequest,
) -> Result<bool, anyhow::Error> {
    let now = chrono::Utc::now();
    let current =
        crate::models::find_sponsor_section(state.database.read(), request.discord_user_id).await?;

    let refund = match &current {
        Some(current) => refund_cents(current, now),
        None => 0,
    };

    if refund > 0 {
        crate::models::add_available_cents(state.database.write(), request.discord_user_id, refund)
            .await?;
    }

    let charged = crate::models::take_available_cents(
        state.database.write(),
        request.discord_user_id,
        request.tier_cents,
    )
    .await?;

    if !charged {
        if refund > 0 {
            crate::models::add_available_cents(
                state.database.write(),
                request.discord_user_id,
                -refund,
            )
            .await?;
        }

        return Ok(false);
    }

    let replaced_logo = current
        .as_ref()
        .and_then(|current| current.logo_path.clone())
        .filter(|path| Some(path) != request.logo_path.as_ref());

    crate::models::upsert_sponsor_section(
        state.database.write(),
        request,
        now,
        now + chrono::Duration::days(PERIOD_DAYS),
    )
    .await?;

    remove_logo(state, replaced_logo.as_deref()).await;

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn section(
        tier_cents: i64,
        period_end: chrono::DateTime<chrono::Utc>,
    ) -> crate::models::SponsorSection {
        crate::models::SponsorSection {
            discord_user_id: 1,
            tier_cents,
            name: "Name".to_string(),
            description: None,
            logo_path: None,
            link: None,
            active: true,
            renewal_warned: false,
            period_start: period_end - chrono::Duration::days(PERIOD_DAYS),
            period_end,
        }
    }

    #[test]
    fn cancelling_gives_back_the_days_that_were_not_used() {
        let now = chrono::Utc::now();

        for tier in SectionTier::ALL {
            let full = section(tier.cents(), now + chrono::Duration::days(PERIOD_DAYS));
            assert_eq!(refund_cents(&full, now), tier.cents());

            let half = section(tier.cents(), now + chrono::Duration::days(PERIOD_DAYS / 2));
            assert_eq!(
                refund_cents(&half, now),
                tier.cents() * (PERIOD_DAYS / 2) / PERIOD_DAYS
            );

            let spent = section(tier.cents(), now);
            assert_eq!(refund_cents(&spent, now), 0);

            // A period the renewal has not caught up with yet cannot pay out more than one.
            let overdue = section(tier.cents(), now - chrono::Duration::days(5));
            assert_eq!(refund_cents(&overdue, now), 0);
        }
    }

    #[test]
    fn a_section_that_is_off_the_website_gives_nothing_back() {
        let now = chrono::Utc::now();
        let mut inactive = section(SectionTier::Link.cents(), now + chrono::Duration::days(30));
        inactive.active = false;

        assert_eq!(refund_cents(&inactive, now), 0);
    }

    #[test]
    fn every_tier_is_priced_apart_and_adds_one_field() {
        let cents: Vec<i64> = SectionTier::ALL
            .into_iter()
            .map(SectionTier::cents)
            .collect();

        assert_eq!(cents, vec![1000, 2000, 5000, 10000]);
        assert!(cents.windows(2).all(|pair| pair[0] < pair[1]));

        for tier in SectionTier::ALL {
            assert_eq!(SectionTier::from_cents(tier.cents()), Some(tier));
        }
        assert_eq!(SectionTier::from_cents(1500), None);

        assert!(!SectionTier::Name.has_description());
        assert!(SectionTier::Description.has_description() && !SectionTier::Description.has_logo());
        assert!(SectionTier::Logo.has_logo() && !SectionTier::Logo.has_link());
        assert!(SectionTier::Link.has_link());
    }
}
