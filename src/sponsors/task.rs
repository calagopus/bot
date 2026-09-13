use super::{Contribution, Evaluation, Ledger, Provider, amount, evaluate, refresh_ledger};
use serenity::all::{
    Component, ContainerComponent, CreateComponent, CreateContainer, CreateContainerComponent,
    CreateMessage, CreateSection, CreateSectionAccessory, CreateSectionComponent,
    CreateTextDisplay, CreateThumbnail, CreateUnfurledMediaItem, EditMessage, GenericChannelId,
    MessageFlags, MessagePagination, Section, SectionComponent, UserId, nonmax::NonMaxU8,
};
use std::collections::HashMap;

const HEADING_ONE_TIME: &str = "## <:cash:1150889514236137605> Sponsorship received";
const HEADING_RECURRING: &str = "## <:cash:1150889514236137605> Recurring sponsorship";
const HEADING_RECURRING_ENDED: &str = "## <:cash:1150889514236137605> Recurring sponsorship ended";

fn sponsor_link(contribution: &Contribution) -> String {
    match &contribution.account {
        Some(account) => format!("[**{}**]({})", account.login, account.url),
        None => "**Someone** (Anonymous)".to_string(),
    }
}

fn avatar_url(env: &crate::env::Env, contribution: &Contribution) -> String {
    match &contribution.account {
        Some(account) => account.avatar_url.clone(),
        None => contribution.provider.fallback_avatar_url(env),
    }
}

fn footer<'a>(env: &crate::env::Env, contribution: &Contribution) -> CreateContainerComponent<'a> {
    CreateContainerComponent::TextDisplay(CreateTextDisplay::new(format!(
        "-# {}",
        contribution.provider.contribute_url(env)
    )))
}

/// A contribution's figures in the currency the announcement messages show.
struct Rendered<'a> {
    currency: &'a str,
    amount_in_cents: i64,
    paid_in_cents: i64,
}

async fn render<'a>(
    env: &'a crate::env::Env,
    contribution: &'a Contribution,
) -> Result<Rendered<'a>, anyhow::Error> {
    let native = contribution.native_currency.as_str();
    let currency = contribution
        .provider
        .message_currency(env)
        .unwrap_or(native);

    Ok(Rendered {
        currency,
        amount_in_cents: super::convert(env, contribution.native_amount_in_cents, native, currency)
            .await?,
        paid_in_cents: super::convert(env, contribution.native_paid_in_cents, native, currency)
            .await?,
    })
}

fn one_time_components<'a>(
    env: &crate::env::Env,
    rendered: &Rendered<'_>,
    contribution: &Contribution,
) -> Vec<CreateContainerComponent<'a>> {
    vec![
        CreateContainerComponent::Section(CreateSection::new(
            vec![
                CreateSectionComponent::TextDisplay(CreateTextDisplay::new(HEADING_ONE_TIME)),
                CreateSectionComponent::TextDisplay(CreateTextDisplay::new(format!(
                    "{} sponsored us for `{}`!",
                    sponsor_link(contribution),
                    amount(rendered.amount_in_cents, rendered.currency)
                ))),
            ],
            CreateSectionAccessory::Thumbnail(CreateThumbnail::new(CreateUnfurledMediaItem::new(
                avatar_url(env, contribution),
            ))),
        )),
        footer(env, contribution),
    ]
}

fn recurring_components<'a>(
    env: &crate::env::Env,
    rendered: &Rendered<'_>,
    contribution: &Contribution,
) -> Vec<CreateContainerComponent<'a>> {
    let months = format!(
        "{} month{}",
        contribution.months_paid,
        if contribution.months_paid == 1 {
            ""
        } else {
            "s"
        }
    );

    let rate = match contribution.interval {
        Some(interval) => format!(
            "{}/{}",
            amount(rendered.amount_in_cents, rendered.currency),
            interval.label()
        ),
        _ => amount(rendered.amount_in_cents, rendered.currency),
    };

    let body = if contribution.active() {
        format!(
            "{} sponsors us for `{}` - `{}` contributed over {}!",
            sponsor_link(contribution),
            rate,
            amount(rendered.paid_in_cents, rendered.currency),
            months
        )
    } else {
        format!(
            "{} sponsored us for `{}` - `{}` contributed over {}.",
            sponsor_link(contribution),
            rate,
            amount(rendered.paid_in_cents, rendered.currency),
            months
        )
    };

    vec![
        CreateContainerComponent::Section(CreateSection::new(
            vec![
                CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                    if contribution.active() {
                        HEADING_RECURRING
                    } else {
                        HEADING_RECURRING_ENDED
                    },
                )),
                CreateSectionComponent::TextDisplay(CreateTextDisplay::new(body)),
            ],
            CreateSectionAccessory::Thumbnail(CreateThumbnail::new(CreateUnfurledMediaItem::new(
                avatar_url(env, contribution),
            ))),
        )),
        footer(env, contribution),
    ]
}

fn message_text(components: &[Component]) -> String {
    let mut text = String::new();

    fn push_section(text: &mut String, section: &Section) {
        for child in section.components.iter() {
            if let SectionComponent::TextDisplay(display) = child {
                text.push_str(&display.content);
                text.push('\n');
            }
        }
    }

    for component in components {
        match component {
            Component::TextDisplay(display) => {
                text.push_str(&display.content);
                text.push('\n');
            }
            Component::Section(section) => push_section(&mut text, section),
            Component::Container(container) => {
                for child in container.components.iter() {
                    match child {
                        ContainerComponent::TextDisplay(display) => {
                            text.push_str(&display.content);
                            text.push('\n');
                        }
                        ContainerComponent::Section(section) => push_section(&mut text, section),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    text
}

async fn backfill_message_ids(
    state: &crate::routes::State,
    channel_id: u64,
    provider: Provider,
    contributions: &[&Contribution],
) -> Result<(), anyhow::Error> {
    let pending = crate::models::sponsorships_without_message(state.database.read(), provider)
        .await?
        .into_iter()
        .filter_map(|sponsorship| {
            contributions
                .iter()
                .find(|contribution| contribution.id == sponsorship.id)
                .map(|contribution| (sponsorship.id, *contribution))
        })
        .collect::<Vec<_>>();

    if pending.is_empty() {
        return Ok(());
    }

    let http = state.bot.read().await.clone();
    let channel_id = GenericChannelId::new(channel_id);
    let current_user = http.get_current_user().await?.id;
    let contribute_url = provider.contribute_url(&state.env);
    let linked = crate::models::linked_message_ids(state.database.read()).await?;

    let mut candidates = Vec::new();
    let mut before = None;

    for _ in 0..5 {
        let messages = http
            .get_messages(
                channel_id,
                before.map(MessagePagination::Before),
                NonMaxU8::new(100),
            )
            .await?;

        if messages.is_empty() {
            break;
        }

        before = messages.last().map(|message| message.id);

        for message in messages {
            if message.author.id != current_user || linked.contains(&(message.id.get() as i64)) {
                continue;
            }

            let text = message_text(&message.components);

            if !text.contains(&contribute_url) {
                continue;
            }

            if text.contains("Sponsorship received")
                || text.contains("Monthly sponsorship")
                || text.contains("Recurring sponsorship")
            {
                candidates.push((message.id, text));
            }
        }

        if before.is_none() {
            break;
        }
    }

    candidates.reverse();

    let mut used = vec![false; candidates.len()];
    let mut linked = 0;

    for (contribution_id, contribution) in pending {
        let marker = contribution
            .account
            .as_ref()
            .map(|account| account.url.clone());

        let found = candidates
            .iter()
            .enumerate()
            .position(|(index, (_, text))| {
                if used[index] {
                    return false;
                }

                match &marker {
                    Some(marker) => text.contains(marker.as_str()),
                    None => text.contains("(Anonymous)"),
                }
            });

        let Some(index) = found else {
            tracing::debug!(
                "no existing message found for sponsorship {}",
                contribution_id
            );
            continue;
        };

        used[index] = true;
        crate::models::set_sent_sponsorship_message(
            state.database.write(),
            &contribution_id,
            candidates[index].0.get() as i64,
        )
        .await?;
        linked += 1;
    }

    if linked > 0 {
        tracing::info!(
            "linked {linked} existing {} sponsorship message(s) for updating",
            provider.key()
        );
    }

    Ok(())
}

async fn ensure_account(
    state: &crate::routes::State,
    contribution: &Contribution,
) -> Result<Option<i64>, anyhow::Error> {
    let Some(account) = contribution.account.as_ref() else {
        return Ok(None);
    };

    let id = crate::models::upsert_linked_account(
        state.database.write(),
        account.provider,
        &account.id,
        Some(&account.login),
    )
    .await?;

    Ok(Some(id))
}

async fn announce(
    state: &crate::routes::State,
    channel_id: u64,
    backfilled: &mut Vec<Provider>,
    provider: Provider,
    evaluation: &Evaluation,
) -> Result<(), anyhow::Error> {
    let announceable = evaluation
        .contributions
        .iter()
        .filter(|contribution| contribution.provider == provider)
        .collect::<Vec<_>>();

    // Seed historical contributions without announcing them, including an empty first poll.
    if !crate::models::is_provider_seeded(state.database.read(), provider).await? {
        tracing::info!(
            "seeding {} existing {} sponsorship(s) without announcing them",
            announceable.len(),
            provider.key()
        );

        for contribution in &announceable {
            let account_id = ensure_account(state, contribution).await?;

            crate::models::insert_sent_sponsorship(
                state.database.write(),
                crate::models::NewSentSponsorship {
                    id: &contribution.id,
                    provider,
                    account_id,
                    amount: Some(contribution.native_amount_in_cents),
                    currency: &contribution.native_currency,
                    message_id: None,
                    recurring: contribution.recurring,
                    paid: None,
                    created: Some(contribution.start),
                },
            )
            .await?;
        }

        crate::models::mark_provider_seeded(state.database.write(), provider).await?;
    }

    let channel = GenericChannelId::new(channel_id);

    for contribution in &announceable {
        let account_id = ensure_account(state, contribution).await?;

        if let Some(sponsorship) =
            crate::models::find_sent_sponsorship(state.database.read(), &contribution.id).await?
        {
            if sponsorship.account_id.is_none() && sponsorship.amount.is_none() {
                crate::models::backfill_sent_sponsorship(
                    state.database.write(),
                    &contribution.id,
                    account_id,
                    Some(contribution.native_amount_in_cents),
                    &contribution.native_currency,
                    contribution.recurring,
                )
                .await?;
            }

            tracing::debug!(
                "sponsorship {} already sent (from: {:?})",
                sponsorship.id,
                sponsorship.created
            );
            continue;
        }

        let rendered = render(&state.env, contribution).await?;

        tracing::info!(
            "new {} {} sponsorship: {} for {}",
            provider.key(),
            if contribution.recurring {
                "recurring"
            } else {
                "one-time"
            },
            contribution
                .account
                .as_ref()
                .map_or("anonymous", |account| account.login.as_str()),
            amount(rendered.amount_in_cents, rendered.currency)
        );

        let components = [CreateComponent::Container(CreateContainer::new(
            if contribution.recurring {
                recurring_components(&state.env, &rendered, contribution)
            } else {
                one_time_components(&state.env, &rendered, contribution)
            },
        ))];
        let message = channel
            .send_message(
                &*state.bot.read().await,
                CreateMessage::new()
                    .components(&components)
                    .flags(MessageFlags::IS_COMPONENTS_V2),
            )
            .await?;

        crate::models::insert_sent_sponsorship(
            state.database.write(),
            crate::models::NewSentSponsorship {
                id: &contribution.id,
                provider,
                account_id,
                amount: Some(contribution.native_amount_in_cents),
                currency: &contribution.native_currency,
                message_id: Some(message.id.get() as i64),
                recurring: contribution.recurring,
                paid: Some(contribution.native_paid_in_cents),
                created: Some(contribution.start),
            },
        )
        .await?;
    }

    if !backfilled.contains(&provider) {
        if let Err(err) = backfill_message_ids(state, channel_id, provider, &announceable).await {
            tracing::error!(
                "failed to link existing {} sponsorship messages: {:?}",
                provider.key(),
                err
            );
        }

        backfilled.push(provider);
    }

    for contribution in announceable
        .iter()
        .filter(|contribution| contribution.recurring)
    {
        let Some(sponsorship) =
            crate::models::find_sent_sponsorship(state.database.read(), &contribution.id).await?
        else {
            continue;
        };

        let Some(message_id) = sponsorship.message_id else {
            continue;
        };

        if sponsorship.currency.as_deref() == Some(contribution.native_currency.as_str())
            && sponsorship.amount == Some(contribution.native_amount_in_cents)
            && sponsorship.paid == Some(contribution.native_paid_in_cents)
            && sponsorship.ended == !contribution.active()
            && sponsorship.recurring
        {
            continue;
        }

        let rendered = render(&state.env, contribution).await?;

        let mut message = state
            .bot
            .read()
            .await
            .get_message(channel, (message_id as u64).into())
            .await?;

        let components = [CreateComponent::Container(CreateContainer::new(
            recurring_components(&state.env, &rendered, contribution),
        ))];
        message
            .edit(
                &*state.bot.read().await,
                EditMessage::new()
                    .components(&components)
                    .flags(MessageFlags::IS_COMPONENTS_V2),
            )
            .await?;

        crate::models::set_sent_sponsorship_rendered(
            state.database.write(),
            &contribution.id,
            contribution.native_amount_in_cents,
            contribution.native_paid_in_cents,
            &contribution.native_currency,
            !contribution.active(),
        )
        .await?;

        tracing::debug!(
            "updated recurring sponsorship message for {} to {}",
            contribution.id,
            amount(rendered.paid_in_cents, rendered.currency)
        );
    }

    Ok(())
}

async fn sync_tier_roles(
    state: &crate::routes::State,
    ledger: &Ledger,
    evaluation: &Evaluation,
) -> Result<(), anyhow::Error> {
    if !crate::tiers::is_configured(&state.env) {
        return Ok(());
    }

    let linked = crate::models::linked_accounts(state.database.read()).await?;
    if linked.is_empty() {
        return Ok(());
    }

    let mut members: HashMap<i64, Vec<(Provider, String)>> = HashMap::new();
    for account in linked {
        let (Some(discord_user_id), Some(provider)) = (account.discord_user_id, account.provider())
        else {
            continue;
        };

        members
            .entry(discord_user_id)
            .or_default()
            .push((provider, account.provider_id));
    }

    let guild_id = crate::utils::guild_id(state).await?;

    for (discord_user_id, accounts) in members {
        if !ledger.is_complete_for(&accounts) {
            continue;
        }

        crate::tiers::apply_roles(
            state,
            guild_id,
            UserId::new(discord_user_id as u64),
            &crate::tiers::earned_role_ids(&state.env, evaluation, &accounts),
        )
        .await?;
    }

    Ok(())
}

async fn run(
    state: &crate::routes::State,
    backfilled: &mut Vec<Provider>,
) -> Result<(), anyhow::Error> {
    let ledger = refresh_ledger(&state.env).await?;
    let evaluation = evaluate(&ledger);

    let mut announced = Ok(());
    if let Some(channel_id) = state.env.sponsors_channel_id {
        for provider_ledger in &ledger.providers {
            let result = announce(
                state,
                channel_id,
                backfilled,
                provider_ledger.provider,
                &evaluation,
            )
            .await;

            if announced.is_ok() {
                announced = result;
            }
        }
    }

    let synced = sync_tier_roles(state, &ledger, &evaluation).await;
    let sections = super::run_sections(state, &ledger, &evaluation).await;

    announced?;
    synced?;
    sections?;

    Ok(())
}

pub fn spawn_sponsor_updates_task(state: crate::routes::State) {
    tokio::spawn(async move {
        let tiers_configured = crate::tiers::is_configured(&state.env);

        if state.env.sponsors_channel_id.is_none()
            && !tiers_configured
            && !super::sections_configured(&state.env)
        {
            return;
        }

        if !super::is_configured(&state.env) {
            tracing::warn!(
                "neither GITHUB_TOKEN nor OPENCOLLECTIVE_SLUG is set, sponsorship updates are disabled"
            );
            return;
        }

        if super::opencollective::is_configured(&state.env)
            && state.env.opencollective_token.is_none()
        {
            tracing::warn!(
                "OPENCOLLECTIVE_TOKEN is not set, open collective requests will be rate limited"
            );
        }

        let mut backfilled = Vec::new();

        loop {
            if let Err(err) = run(&state, &mut backfilled).await {
                tracing::error!("failed to collect sponsors: {:?}", err);
                sentry_anyhow::capture_anyhow(&err);
            }

            tokio::time::sleep(std::time::Duration::from_mins(5)).await;
        }
    });
}
