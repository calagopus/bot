use super::{EvaluatedSpell, SponsorsActivityAction, evaluate, refresh_ledger, sponsors_login};
use serenity::all::{
    Component, ContainerComponent, CreateComponent, CreateContainer, CreateContainerComponent,
    CreateMessage, CreateSection, CreateSectionAccessory, CreateSectionComponent,
    CreateTextDisplay, CreateThumbnail, CreateUnfurledMediaItem, EditMessage, GenericChannelId,
    MessageFlags, MessagePagination, Section, SectionComponent, nonmax::NonMaxU8,
};

const HEADING_ONE_TIME: &str = "## <:cash:1150889514236137605> Sponsorship received";
const HEADING_MONTHLY: &str = "## <:cash:1150889514236137605> Monthly sponsorship";
const HEADING_MONTHLY_ENDED: &str = "## <:cash:1150889514236137605> Monthly sponsorship ended";

fn dollars(cents: i64) -> String {
    format!("${:.2}", cents as f64 / 100.0)
}

fn sponsor_link(spell: &EvaluatedSpell) -> String {
    match &spell.sponsor {
        Some(sponsor) => format!(
            "[**{login}**](https://github.com/{login})",
            login = sponsor.login
        ),
        None => "**Someone** (Anonymous)".to_string(),
    }
}

fn avatar_url(env: &crate::env::Env, spell: &EvaluatedSpell) -> String {
    match &spell.sponsor {
        Some(sponsor) => sponsor.avatar_url.clone(),
        None => format!("https://github.com/{}.png", sponsors_login(env)),
    }
}

fn one_time_components<'a>(
    env: &crate::env::Env,
    spell_like: &EvaluatedSpell,
) -> Vec<CreateContainerComponent<'a>> {
    vec![
        CreateContainerComponent::Section(CreateSection::new(
            vec![
                CreateSectionComponent::TextDisplay(CreateTextDisplay::new(HEADING_ONE_TIME)),
                CreateSectionComponent::TextDisplay(CreateTextDisplay::new(format!(
                    "{} sponsored us for `{}`!",
                    sponsor_link(spell_like),
                    dollars(spell_like.monthly_in_cents)
                ))),
            ],
            CreateSectionAccessory::Thumbnail(CreateThumbnail::new(CreateUnfurledMediaItem::new(
                avatar_url(env, spell_like),
            ))),
        )),
        CreateContainerComponent::TextDisplay(CreateTextDisplay::new(format!(
            "-# https://github.com/sponsors/{}",
            sponsors_login(env)
        ))),
    ]
}

fn monthly_components<'a>(
    env: &crate::env::Env,
    spell: &EvaluatedSpell,
) -> Vec<CreateContainerComponent<'a>> {
    let months = format!(
        "{} month{}",
        spell.months_paid,
        if spell.months_paid == 1 { "" } else { "s" }
    );

    let body = if spell.active() {
        format!(
            "{} sponsors us monthly for `{}` - `{}` contributed over {}!",
            sponsor_link(spell),
            dollars(spell.monthly_in_cents),
            dollars(spell.paid_in_cents),
            months
        )
    } else {
        format!(
            "{} sponsored us monthly for `{}` - `{}` contributed over {}.",
            sponsor_link(spell),
            dollars(spell.monthly_in_cents),
            dollars(spell.paid_in_cents),
            months
        )
    };

    vec![
        CreateContainerComponent::Section(CreateSection::new(
            vec![
                CreateSectionComponent::TextDisplay(CreateTextDisplay::new(if spell.active() {
                    HEADING_MONTHLY
                } else {
                    HEADING_MONTHLY_ENDED
                })),
                CreateSectionComponent::TextDisplay(CreateTextDisplay::new(body)),
            ],
            CreateSectionAccessory::Thumbnail(CreateThumbnail::new(CreateUnfurledMediaItem::new(
                avatar_url(env, spell),
            ))),
        )),
        CreateContainerComponent::TextDisplay(CreateTextDisplay::new(format!(
            "-# https://github.com/sponsors/{}",
            sponsors_login(env)
        ))),
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
    spells: &[EvaluatedSpell],
) -> Result<(), anyhow::Error> {
    let pending = crate::models::sponsorships_without_message(state.database.read())
        .await?
        .into_iter()
        .filter_map(|sponsorship| {
            spells
                .iter()
                .find(|spell| spell.activity_id == sponsorship.id)
                .map(|spell| (sponsorship.id, spell))
        })
        .collect::<Vec<_>>();

    if pending.is_empty() {
        return Ok(());
    }

    let http = state.bot.read().await.clone();
    let channel_id = GenericChannelId::new(channel_id);
    let current_user = http.get_current_user().await?.id;

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
            if message.author.id != current_user {
                continue;
            }

            let text = message_text(&message.components);
            if text.contains("Sponsorship received") || text.contains("Monthly sponsorship") {
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

    for (activity_id, spell) in pending {
        let marker = spell
            .sponsor
            .as_ref()
            .map(|sponsor| format!("https://github.com/{}", sponsor.login));

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
                "no existing message found for recurring sponsorship {}",
                activity_id
            );
            continue;
        };

        used[index] = true;
        crate::models::set_sent_sponsorship_message(
            state.database.write(),
            &activity_id,
            candidates[index].0.get() as i64,
        )
        .await?;
        linked += 1;
    }

    if linked > 0 {
        tracing::info!("linked {linked} existing sponsorship message(s) for updating");
    }

    Ok(())
}

async fn run(
    state: &crate::routes::State,
    channel_id: u64,
    backfilled: &mut bool,
) -> Result<(), anyhow::Error> {
    let ledger = refresh_ledger(&state.env).await?;
    let evaluation = evaluate(&ledger, chrono::Utc::now());

    let announceable = ledger
        .activities
        .iter()
        .filter(|activity| activity.action == Some(SponsorsActivityAction::NewSponsorship))
        .collect::<Vec<_>>();

    if crate::models::count_sent_sponsorships(state.database.read()).await? == 0 {
        tracing::info!(
            "seeding {} existing sponsorship(s) without announcing them",
            announceable.len()
        );

        for activity in &announceable {
            let recurring = activity
                .sponsors_tier
                .as_ref()
                .is_some_and(|tier| !tier.is_one_time);

            crate::models::insert_sent_sponsorship(
                state.database.write(),
                crate::models::NewSentSponsorship {
                    id: &activity.id,
                    github_id: activity.sponsor.as_ref().and_then(|s| s.database_id),
                    amount: activity
                        .sponsors_tier
                        .as_ref()
                        .map(|tier| tier.monthly_price_in_cents),
                    recurring,
                    created: activity.timestamp,
                    ..Default::default()
                },
            )
            .await?;
        }
    }

    let channel = GenericChannelId::new(channel_id);

    for activity in announceable {
        let recurring = activity
            .sponsors_tier
            .as_ref()
            .is_some_and(|tier| !tier.is_one_time);
        let amount_in_cents = activity
            .sponsors_tier
            .as_ref()
            .map(|tier| tier.monthly_price_in_cents);

        if let Some(sponsorship) =
            crate::models::find_sent_sponsorship(state.database.read(), &activity.id).await?
        {
            if sponsorship.github_id.is_none() && sponsorship.amount.is_none() {
                crate::models::backfill_sent_sponsorship(
                    state.database.write(),
                    &activity.id,
                    activity.sponsor.as_ref().and_then(|s| s.database_id),
                    amount_in_cents,
                    recurring,
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

        let synthesised;
        let spell = match evaluation
            .spells
            .iter()
            .find(|spell| spell.activity_id == activity.id)
        {
            Some(spell) => spell,
            None => {
                synthesised = EvaluatedSpell {
                    activity_id: activity.id.clone(),
                    sponsor: activity
                        .is_public()
                        .then(|| activity.sponsor.clone())
                        .flatten(),
                    start: activity.timestamp.unwrap_or_else(chrono::Utc::now),
                    end: None,
                    monthly_in_cents: amount_in_cents.unwrap_or(0),
                    paid_in_cents: amount_in_cents.unwrap_or(0),
                    months_paid: 1,
                };

                &synthesised
            }
        };

        tracing::info!(
            "new {} sponsorship: {} for {}",
            if recurring { "monthly" } else { "one-time" },
            spell
                .sponsor
                .as_ref()
                .map_or("anonymous", |sponsor| sponsor.login.as_str()),
            dollars(spell.monthly_in_cents)
        );

        let components = [CreateComponent::Container(CreateContainer::new(
            if recurring {
                monthly_components(&state.env, spell)
            } else {
                one_time_components(&state.env, spell)
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
                id: &activity.id,
                github_id: activity.sponsor.as_ref().and_then(|s| s.database_id),
                amount: amount_in_cents,
                message_id: Some(message.id.get() as i64),
                recurring,
                paid: Some(spell.paid_in_cents),
                created: activity.timestamp,
            },
        )
        .await?;
    }

    if !*backfilled {
        if let Err(err) = backfill_message_ids(state, channel_id, &evaluation.spells).await {
            tracing::error!("failed to link existing sponsorship messages: {:?}", err);
        }

        *backfilled = true;
    }

    for spell in &evaluation.spells {
        let Some(sponsorship) =
            crate::models::find_sent_sponsorship(state.database.read(), &spell.activity_id).await?
        else {
            continue;
        };

        let Some(message_id) = sponsorship.message_id else {
            continue;
        };

        if sponsorship.paid == Some(spell.paid_in_cents)
            && sponsorship.ended == !spell.active()
            && sponsorship.recurring
        {
            continue;
        }

        let mut message = state
            .bot
            .read()
            .await
            .get_message(channel, (message_id as u64).into())
            .await?;

        let components = [CreateComponent::Container(CreateContainer::new(
            monthly_components(&state.env, spell),
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
            &spell.activity_id,
            spell.paid_in_cents,
            !spell.active(),
        )
        .await?;

        tracing::debug!(
            "updated monthly sponsorship message for {} to {}",
            spell.activity_id,
            dollars(spell.paid_in_cents)
        );
    }

    Ok(())
}

pub fn spawn_sponsor_updates_task(state: crate::routes::State) {
    tokio::spawn(async move {
        let Some(channel_id) = state.env.github_sponsors_channel_id else {
            return;
        };

        if state.env.github_token.is_none() {
            tracing::warn!("GITHUB_TOKEN is not set, sponsorship updates are disabled");
            return;
        }

        let mut backfilled = false;

        loop {
            if let Err(err) = run(&state, channel_id, &mut backfilled).await {
                tracing::error!("failed to collect sponsors: {:?}", err);
                sentry_anyhow::capture_anyhow(&err);
            }

            tokio::time::sleep(std::time::Duration::from_mins(5)).await;
        }
    });
}
