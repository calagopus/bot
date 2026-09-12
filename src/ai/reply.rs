use super::agent::Outcome;
use serenity::all::{GenericChannelId, nonmax::NonMaxU8};
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
};

const HISTORY_LIMIT: u8 = 40;
/// How long a thread must stay quiet before the model is given the conversation, so someone
/// sending three messages in a row is answered once with all three in view.
const QUIET_PERIOD: std::time::Duration = std::time::Duration::from_secs(6);
/// Ceiling on that wait, for a thread nobody stops typing in.
const MAX_QUIET_WAIT: std::time::Duration = std::time::Duration::from_secs(45);

#[derive(Default)]
struct Queued {
    latest: Option<serenity::all::Message>,
    forced: bool,
    active: bool,
}

static QUEUE: LazyLock<Arc<tokio::sync::Mutex<HashMap<u64, Queued>>>> =
    LazyLock::new(|| Arc::new(tokio::sync::Mutex::new(HashMap::new())));

async fn loading_message(
    http: &serenity::http::Http,
    message: &serenity::all::Message,
) -> Option<serenity::all::Message> {
    message
        .channel_id
        .send_message(
            http,
            serenity::all::CreateMessage::new()
                .content("-# Loading...")
                .reference_message(message)
                .allowed_mentions(serenity::all::CreateAllowedMentions::new().replied_user(true))
                .flags(serenity::all::MessageFlags::SUPPRESS_EMBEDS),
        )
        .await
        .ok()
}

async fn discard(http: &serenity::http::Http, loading: Option<serenity::all::Message>) {
    if let Some(loading) = loading
        && let Err(err) = loading.delete(http, None).await
    {
        tracing::debug!("ai support could not remove its loading message: {err}");
    }
}

async fn escalate(
    state: &crate::routes::State,
    http: &serenity::http::Http,
    channel_id: GenericChannelId,
) -> Result<(), anyhow::Error> {
    let roles = state
        .env
        .ai_support_role_ids
        .iter()
        .map(|role_id| serenity::all::RoleId::new(*role_id))
        .collect::<Vec<_>>();

    let mut content = roles
        .iter()
        .map(|role_id| format!("<@&{role_id}>"))
        .collect::<Vec<_>>()
        .join(" ");
    if !content.is_empty() {
        content.push('\n');
    }
    content.push_str(
        "-# An answer was drafted here but held back by a safety check, so this one needs a person.",
    );

    channel_id
        .send_message(
            http,
            serenity::all::CreateMessage::new()
                .content(content)
                .allowed_mentions(serenity::all::CreateAllowedMentions::new().roles(roles))
                .flags(serenity::all::MessageFlags::SUPPRESS_EMBEDS),
        )
        .await?;

    Ok(())
}

async fn post_answer(
    http: &serenity::http::Http,
    message: &serenity::all::Message,
    loading: Option<serenity::all::Message>,
    content: &str,
    mut attachments: Vec<serenity::all::CreateAttachment<'_>>,
) -> Result<(), anyhow::Error> {
    let mentions = serenity::all::CreateAllowedMentions::new().replied_user(true);
    let mut loading = loading;

    for (index, part) in super::render::split(content).into_iter().enumerate() {
        if index == 0
            && let Some(mut loading) = loading.take()
        {
            let mut edit = serenity::all::EditMessage::new()
                .content(part)
                .allowed_mentions(mentions.clone())
                .flags(serenity::all::MessageFlags::SUPPRESS_EMBEDS);

            for attachment in attachments.drain(..) {
                edit = edit.new_attachment(attachment);
            }

            loading.edit(http, edit).await?;

            continue;
        }

        let mut create = serenity::all::CreateMessage::new()
            .content(part)
            .allowed_mentions(mentions.clone())
            .flags(serenity::all::MessageFlags::SUPPRESS_EMBEDS)
            .add_files(attachments.drain(..));

        if index == 0 {
            create = create.reference_message(message);
        }

        message.channel_id.send_message(http, create).await?;
    }

    Ok(())
}

async fn answer(
    state: &crate::routes::State,
    message: &serenity::all::Message,
    triage: bool,
) -> Result<(), anyhow::Error> {
    let http = state.bot.read().await.clone();
    let channel_id = message.channel_id;

    let Some(thread) = http.get_channel(channel_id).await?.thread() else {
        return Ok(());
    };

    let mut history = http
        .get_messages(channel_id, None, NonMaxU8::new(HISTORY_LIMIT))
        .await?;
    history.reverse();

    let context = super::transcript::transcript(
        http.get_current_user().await?.id,
        &thread.base.name,
        &history,
    )
    .await;

    if triage && !super::agent::needs_reply(&state.env, &context.text).await? {
        tracing::debug!(thread = %thread.id, "ai support triage skipped a message");

        return Ok(());
    }

    let loading = loading_message(&http, message).await;

    let answer = match super::agent::generate(&state.env, &context).await {
        Ok(Outcome::Answer(answer)) => answer,
        Ok(Outcome::Skip) => {
            discard(&http, loading).await;

            return Ok(());
        }
        Ok(Outcome::HandOver(reason)) => {
            discard(&http, loading).await;

            return super::support::hand_over(state, &http, thread.id.get() as i64, &reason).await;
        }
        Err(err) => {
            discard(&http, loading).await;

            return Err(err);
        }
    };

    if let Some(block) = super::guard::check(&answer.text, !answer.sources.is_empty()) {
        tracing::warn!(
            thread = %thread.id,
            reason = %block.reason(),
            draft = %answer.text,
            "ai support held back an answer"
        );
        sentry::capture_message(
            &format!("ai support held back an answer: {}", block.reason()),
            sentry::Level::Warning,
        );

        discard(&http, loading).await;
        escalate(state, &http, channel_id).await?;

        return super::support::pause(state, thread.id.get() as i64).await;
    }

    tracing::info!(
        thread = %thread.id,
        sources = answer.sources.len(),
        dropped = answer.dropped,
        "ai support answered a message"
    );

    let attachments = super::render::documentation_images(&state.env, &answer.images).await;
    let content = super::render::render(super::docs_url(&state.env), &answer);

    post_answer(&http, message, loading, &content, attachments).await
}

async fn latest(thread_id: u64) -> Option<serenity::all::MessageId> {
    QUEUE
        .lock()
        .await
        .get(&thread_id)
        .and_then(|queued| queued.latest.as_ref())
        .map(|message| message.id)
}

async fn drain(state: &crate::routes::State, thread_id: u64) -> Result<(), anyhow::Error> {
    loop {
        let started = std::time::Instant::now();
        let mut seen = latest(thread_id).await;

        while started.elapsed() < MAX_QUIET_WAIT {
            tokio::time::sleep(QUIET_PERIOD).await;

            let newest = latest(thread_id).await;

            if newest == seen {
                break;
            }

            seen = newest;
        }

        let (message, forced) = {
            let mut queue = QUEUE.lock().await;
            let Some(queued) = queue.get_mut(&thread_id) else {
                return Ok(());
            };

            (queued.latest.take(), std::mem::take(&mut queued.forced))
        };

        let Some(message) = message else {
            return Ok(());
        };

        let result = answer(state, &message, !forced).await;

        if latest(thread_id).await.is_none() {
            return result;
        }

        result?;
    }
}

async fn handle_message(
    state: &crate::routes::State,
    message: serenity::all::Message,
    triage: bool,
) -> Result<(), anyhow::Error> {
    let thread_id = message.channel_id.get();

    {
        let mut queue = QUEUE.lock().await;
        let queued = queue.entry(thread_id).or_default();

        queued.latest = Some(message);
        queued.forced |= !triage;

        if queued.active {
            return Ok(());
        }

        queued.active = true;
    }

    let result = drain(state, thread_id).await;

    QUEUE.lock().await.remove(&thread_id);

    result
}

pub fn spawn_enabled_answer(state: &crate::routes::State, thread_id: i64) {
    let state = state.clone();

    tokio::spawn(async move {
        let run = async {
            let http = state.bot.read().await.clone();
            let channel_id = GenericChannelId::new(thread_id as u64);
            let bot_id = http.get_current_user().await?.id;

            let history = http
                .get_messages(channel_id, None, NonMaxU8::new(HISTORY_LIMIT))
                .await?;

            let target = match history
                .iter()
                .find(|message| !message.author.bot() && message.author.id != bot_id)
            {
                Some(message) => message.clone(),
                None => {
                    http.get_message(channel_id, (thread_id as u64).into())
                        .await?
                }
            };

            let answered = history.iter().any(|message| {
                message.author.id == bot_id
                    && message
                        .message_reference
                        .as_ref()
                        .is_some_and(|reference| reference.message_id == Some(target.id))
            });

            if answered {
                tracing::debug!(
                    thread = thread_id,
                    "ai support was switched on with nothing new to answer"
                );

                return Ok(());
            }

            handle_message(&state, target, false).await
        };

        if let Err(err) = run.await {
            tracing::error!("ai support failed to answer after being enabled: {:?}", err);
            sentry_anyhow::capture_anyhow(&err);
        }
    });
}

pub fn spawn_answer(state: &crate::routes::State, message: &serenity::all::Message) {
    let state = state.clone();
    let message = message.clone();

    tokio::spawn(async move {
        if let Err(err) = handle_message(&state, message, true).await {
            tracing::error!("ai support failed to answer: {:?}", err);
            sentry_anyhow::capture_anyhow(&err);
        }
    });
}
