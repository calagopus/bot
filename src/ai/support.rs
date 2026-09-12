use serenity::all::GenericChannelId;

const MAX_TAGS: usize = 5;

fn notice(content: &str) -> serenity::all::CreateMessage<'_> {
    serenity::all::CreateMessage::new()
        .content(content)
        .allowed_mentions(serenity::all::CreateAllowedMentions::new())
        .flags(serenity::all::MessageFlags::SUPPRESS_EMBEDS)
}

async fn update_control_message(
    state: &crate::routes::State,
    thread: &crate::models::AiSupportThread,
) -> Result<(), anyhow::Error> {
    let Some(message_id) = thread.message_id else {
        return Ok(());
    };

    let http = state.bot.read().await.clone();
    let mut message = http
        .get_message(
            GenericChannelId::new(thread.thread_id as u64),
            (message_id as u64).into(),
        )
        .await?;

    message
        .edit(
            &http,
            serenity::all::EditMessage::new()
                .components(crate::components::ai_support::control_components(thread))
                .flags(serenity::all::MessageFlags::IS_COMPONENTS_V2),
        )
        .await?;

    Ok(())
}

pub async fn pause(state: &crate::routes::State, thread_id: i64) -> Result<(), anyhow::Error> {
    let Some(mut thread) =
        crate::models::find_ai_support_thread(state.database.read(), thread_id).await?
    else {
        return Ok(());
    };

    crate::models::set_ai_support_paused(state.database.write(), thread_id, true).await?;
    thread.paused = true;

    update_control_message(state, &thread).await
}

pub async fn hand_over(
    state: &crate::routes::State,
    http: &serenity::http::Http,
    thread_id: i64,
    reason: &str,
) -> Result<(), anyhow::Error> {
    let Some(mut thread) =
        crate::models::find_ai_support_thread(state.database.read(), thread_id).await?
    else {
        return Ok(());
    };

    crate::models::set_ai_support_enabled(state.database.write(), thread_id, false).await?;
    thread.enabled = false;
    thread.paused = false;
    update_control_message(state, &thread).await?;

    tracing::info!(thread = thread_id, reason = %reason, "ai support handed a thread over");

    GenericChannelId::new(thread_id as u64)
        .send_message(
            http,
            notice(
                "-# I do not think I can answer this one properly, so I have switched myself off here and left it for the team. Use the button above if you want me to try anyway.",
            ),
        )
        .await?;

    Ok(())
}

pub async fn handle_thread_create(
    state: &crate::routes::State,
    ctx: &serenity::prelude::Context,
    thread: &serenity::all::GuildThread,
) -> Result<(), anyhow::Error> {
    if !super::is_configured(&state.env)
        || state.env.ai_support_channel_id != Some(thread.parent_id.get())
    {
        return Ok(());
    }

    let thread_id = thread.id.get() as i64;

    if crate::models::find_ai_support_thread(state.database.read(), thread_id)
        .await?
        .is_some()
    {
        return Ok(());
    }

    if let Some(tag_id) = state.env.ai_support_unresolved_tag_id {
        let tag = serenity::all::ForumTagId::new(tag_id);

        if !thread.applied_tags.contains(&tag) {
            let mut tags: Vec<serenity::all::ForumTagId> =
                thread.applied_tags.iter().copied().collect();
            tags.truncate(MAX_TAGS - 1);
            tags.push(tag);

            serenity::all::EditThread::new()
                .applied_tags(tags)
                .audit_log_reason("new support post")
                .execute(&ctx.http, thread.id)
                .await?;
        }
    }

    let record = crate::models::AiSupportThread {
        thread_id,
        message_id: None,
        owner_id: Some(thread.owner_id.get() as i64),
        enabled: false,
        paused: false,
        resolved: false,
    };

    let message = thread
        .send_message(
            &ctx.http,
            serenity::all::CreateMessage::new()
                .components(crate::components::ai_support::control_components(&record))
                .flags(serenity::all::MessageFlags::IS_COMPONENTS_V2),
        )
        .await?;

    crate::models::insert_ai_support_thread(
        state.database.write(),
        thread_id,
        message.id.get() as i64,
        thread.owner_id.get() as i64,
    )
    .await?;

    tracing::info!(thread = thread_id, "ai support offered on a new post");

    Ok(())
}

async fn yield_to_team(
    state: &crate::routes::State,
    message: &serenity::all::Message,
    thread: &mut crate::models::AiSupportThread,
) -> Result<(), anyhow::Error> {
    if !thread.enabled || thread.paused {
        return Ok(());
    }

    crate::models::set_ai_support_paused(state.database.write(), thread.thread_id, true).await?;
    thread.paused = true;
    update_control_message(state, thread).await?;

    message
        .channel_id
        .send_message(
            &*state.bot.read().await,
            notice(
                "-# AI support paused here, the team has this one. Use the button above to turn it back on.",
            ),
        )
        .await?;

    tracing::info!(
        thread = thread.thread_id,
        "ai support paused after a team member replied"
    );

    Ok(())
}

pub async fn handle_message_create(
    state: &crate::routes::State,
    message: &serenity::all::Message,
) -> Result<(), anyhow::Error> {
    if !super::is_configured(&state.env) {
        return Ok(());
    }

    if message.id.get() == message.channel_id.get() {
        return Ok(());
    }

    let Some(mut thread) = crate::models::find_ai_support_thread(
        state.database.read(),
        message.channel_id.get() as i64,
    )
    .await?
    else {
        return Ok(());
    };

    if thread.resolved {
        return Ok(());
    }

    let author = thread.owner_id == Some(message.author.id.get() as i64);
    let staff = !author
        && message
            .member
            .as_ref()
            .is_some_and(|member| super::is_staff(&state.env, &member.roles));

    if staff {
        return yield_to_team(state, message, &mut thread).await;
    }

    if !thread.answers() {
        return Ok(());
    }

    super::reply::spawn_answer(state, message);

    Ok(())
}
