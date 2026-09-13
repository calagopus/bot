use crate::{
    modals::sponsor_section::SponsorSectionRevokeModal,
    sponsors::{SectionTier, amount},
};
use poise::Modal;
use serenity::all::{
    ComponentInteraction, ComponentInteractionDataKind, CreateInteractionResponse,
    CreateInteractionResponseMessage, EditInteractionResponse, GenericChannelId, MessageFlags,
    ModalInteraction,
};

const LOGO_CONTENT_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/webp", "image/gif"];
const LOGO_MAX_BYTES: u32 = 2 * 1024 * 1024;

async fn respond(
    ctx: &serenity::prelude::Context,
    interaction: &ComponentInteraction,
    content: &str,
) -> Result<(), anyhow::Error> {
    interaction
        .create_response(
            &ctx.http,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content(content.to_string())
                    .ephemeral(true),
            ),
        )
        .await?;

    Ok(())
}

fn is_moderator(state: &crate::routes::State, channel_id: GenericChannelId) -> bool {
    state.env.sponsor_sections_channel_id == Some(channel_id.get())
}

async fn spendable_cents(
    state: &crate::routes::State,
    discord_user_id: i64,
) -> Result<i64, anyhow::Error> {
    let available = crate::models::available_cents(state.database.read(), discord_user_id).await?;
    let section =
        crate::models::find_sponsor_section(state.database.read(), discord_user_id).await?;

    Ok(available
        + section
            .map(|section| crate::sponsors::refund_cents(&section, chrono::Utc::now()))
            .unwrap_or(0))
}

pub struct SponsorSectionBuy;

#[async_trait::async_trait]
impl crate::components::Component for SponsorSectionBuy {
    async fn execute(
        &self,
        state: &crate::routes::State,
        ctx: &serenity::prelude::Context,
        interaction: &ComponentInteraction,
    ) -> Result<Option<()>, anyhow::Error> {
        let ComponentInteractionDataKind::StringSelect { values, .. } = &interaction.data.kind
        else {
            return Ok(None);
        };

        if interaction.data.custom_id != super::SPONSOR_SECTION_BUY_ID {
            return Ok(None);
        }

        let Some(tier) = values
            .first()
            .and_then(|cents| cents.parse().ok())
            .and_then(SectionTier::from_cents)
        else {
            return Ok(None);
        };

        if !crate::sponsors::sections_configured(&state.env) {
            respond(ctx, interaction, "Sponsor sections are not configured.").await?;
            return Ok(Some(()));
        }

        let discord_user_id = interaction.user.id.get() as i64;

        if crate::models::find_pending_sponsor_section_request(
            state.database.read(),
            discord_user_id,
        )
        .await?
        .is_some()
        {
            respond(
                ctx,
                interaction,
                "You already have a change waiting for a moderator. Wait for it to be approved or denied first.",
            )
            .await?;
            return Ok(Some(()));
        }

        let spendable = spendable_cents(state, discord_user_id).await?;
        if spendable < tier.cents() {
            respond(
                ctx,
                interaction,
                &format!(
                    "That tier costs `{}` and you have `{}` left to spend.",
                    amount(tier.cents(), &state.env.sponsors_currency),
                    amount(spendable, &state.env.sponsors_currency)
                ),
            )
            .await?;
            return Ok(Some(()));
        }

        let section =
            crate::models::find_sponsor_section(state.database.read(), discord_user_id).await?;

        interaction
            .create_response(
                &ctx.http,
                crate::modals::sponsor_section::create(
                    tier,
                    section.as_ref(),
                    format!("{}{}", super::SPONSOR_SECTION_MODAL_ID, tier.cents()),
                ),
            )
            .await?;

        Ok(Some(()))
    }
}

async fn store_logo(
    state: &crate::routes::State,
    discord_user_id: i64,
    logo: &serenity::all::Attachment,
) -> Result<Result<String, &'static str>, anyhow::Error> {
    let Some(storage) = state.storage.as_ref() else {
        return Ok(Err("Logo storage is not configured."));
    };

    let Some(content_type) = logo
        .content_type
        .as_deref()
        .map(|content_type| content_type.split(';').next().unwrap_or_default().trim())
        .filter(|content_type| LOGO_CONTENT_TYPES.contains(content_type))
    else {
        return Ok(Err("A logo has to be a png, jpeg, webp or gif."));
    };

    if logo.size > LOGO_MAX_BYTES {
        return Ok(Err("A logo has to be 2 MiB or smaller."));
    }

    let extension = match content_type {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        _ => "gif",
    };
    let path = format!(
        "sponsor-sections/{discord_user_id}/{}.{extension}",
        chrono::Utc::now().timestamp()
    );

    storage
        .store(&path, logo.download().await?, content_type)
        .await?;

    Ok(Ok(path))
}

pub async fn submit(
    state: &crate::routes::State,
    ctx: &serenity::prelude::Context,
    interaction: &ModalInteraction,
) -> Result<Option<()>, anyhow::Error> {
    let Some(tier) = interaction
        .data
        .custom_id
        .strip_prefix(super::SPONSOR_SECTION_MODAL_ID)
        .and_then(|cents| cents.parse().ok())
        .and_then(SectionTier::from_cents)
    else {
        return Ok(None);
    };

    interaction.defer_ephemeral(&ctx.http).await?;

    let discord_user_id = interaction.user.id.get() as i64;
    let input =
        crate::modals::sponsor_section::SponsorSectionInput::parse(tier, interaction.data.clone());

    let reply = async |content: String| -> Result<(), anyhow::Error> {
        interaction
            .edit_response(&ctx.http, EditInteractionResponse::new().content(content))
            .await?;

        Ok(())
    };

    if let Some(link) = &input.link
        && !link.starts_with("https://")
        && !link.starts_with("http://")
    {
        reply("A link has to start with `https://`.".to_string()).await?;
        return Ok(Some(()));
    }

    let section =
        crate::models::find_sponsor_section(state.database.read(), discord_user_id).await?;

    let logo_path = match (&input.logo, tier.has_logo()) {
        (Some(logo), true) => match store_logo(state, discord_user_id, logo).await? {
            Ok(path) => Some(path),
            Err(message) => {
                reply(message.to_string()).await?;
                return Ok(Some(()));
            }
        },
        (_, true) => match section
            .as_ref()
            .and_then(|section| section.logo_path.clone())
        {
            Some(path) => Some(path),
            None => {
                reply("That tier needs a logo.".to_string()).await?;
                return Ok(Some(()));
            }
        },
        _ => None,
    };

    let request = crate::models::insert_sponsor_section_request(
        state.database.write(),
        crate::models::NewSponsorSectionRequest {
            discord_user_id,
            tier_cents: tier.cents(),
            name: &input.name,
            description: input.description.as_deref(),
            logo_path: logo_path.as_deref(),
            link: input.link.as_deref(),
        },
    )
    .await;

    let request = match request {
        Ok(request) => request,
        Err(err) => {
            if logo_path.as_ref()
                != section
                    .as_ref()
                    .and_then(|section| section.logo_path.as_ref())
            {
                crate::sponsors::remove_logo(state, logo_path.as_deref()).await;
            }

            tracing::debug!("sponsor section request rejected: {:?}", err);

            reply(
                "You already have a change waiting for a moderator. Wait for it to be approved or denied first."
                    .to_string(),
            )
            .await?;
            return Ok(Some(()));
        }
    };

    crate::sponsors::post_request(state, &request).await?;

    reply(format!(
        "Your section was sent to the moderators. Once it is approved, `{}` is taken from your funds and it runs for {} days.",
        amount(tier.cents(), &state.env.sponsors_currency),
        crate::sponsors::PERIOD_DAYS
    ))
    .await?;

    tracing::info!(
        user = %interaction.user.name,
        tier = tier.cents(),
        "sponsor section change requested"
    );

    Ok(Some(()))
}

pub struct SponsorSectionCancel;

#[async_trait::async_trait]
impl crate::components::Component for SponsorSectionCancel {
    async fn execute(
        &self,
        state: &crate::routes::State,
        ctx: &serenity::prelude::Context,
        interaction: &ComponentInteraction,
    ) -> Result<Option<()>, anyhow::Error> {
        if !matches!(interaction.data.kind, ComponentInteractionDataKind::Button)
            || interaction.data.custom_id != super::SPONSOR_SECTION_CANCEL_ID
        {
            return Ok(None);
        }

        let discord_user_id = interaction.user.id.get() as i64;
        let section =
            crate::models::find_sponsor_section(state.database.read(), discord_user_id).await?;

        let Some(section) = section.filter(|section| section.active) else {
            respond(
                ctx,
                interaction,
                "You do not have a section on the website.",
            )
            .await?;
            return Ok(Some(()));
        };

        let refunded = crate::sponsors::end_section(state, &section).await?;

        interaction
            .create_response(
                &ctx.http,
                CreateInteractionResponse::UpdateMessage(
                    CreateInteractionResponseMessage::new()
                        .components(
                            crate::commands::sponsor::section_components(state, discord_user_id)
                                .await?,
                        )
                        .flags(MessageFlags::IS_COMPONENTS_V2),
                ),
            )
            .await?;

        interaction
            .create_followup(
                &ctx.http,
                serenity::all::CreateInteractionResponseFollowup::new()
                    .content(format!(
                        "Your section was taken off the website and `{}` went back to your funds.",
                        amount(refunded, &state.env.sponsors_currency)
                    ))
                    .ephemeral(true),
            )
            .await?;

        tracing::info!(
            user = %interaction.user.name,
            refunded,
            "sponsor section cancelled"
        );

        Ok(Some(()))
    }
}

pub struct SponsorSectionDecide;

#[async_trait::async_trait]
impl crate::components::Component for SponsorSectionDecide {
    async fn execute(
        &self,
        state: &crate::routes::State,
        ctx: &serenity::prelude::Context,
        interaction: &ComponentInteraction,
    ) -> Result<Option<()>, anyhow::Error> {
        if !matches!(interaction.data.kind, ComponentInteractionDataKind::Button) {
            return Ok(None);
        }

        let custom_id = interaction.data.custom_id.as_str();
        let (approved, id) = match (
            custom_id.strip_prefix(super::SPONSOR_SECTION_APPROVE_ID),
            custom_id.strip_prefix(super::SPONSOR_SECTION_DENY_ID),
        ) {
            (Some(id), _) => (true, id),
            (_, Some(id)) => (false, id),
            _ => return Ok(None),
        };

        let Some(id) = id.parse().ok() else {
            return Ok(None);
        };

        if !is_moderator(state, interaction.channel_id) {
            respond(
                ctx,
                interaction,
                "Sponsor sections are only decided in the moderator channel.",
            )
            .await?;
            return Ok(Some(()));
        }

        let Some(request) =
            crate::models::find_sponsor_section_request(state.database.read(), id).await?
        else {
            respond(ctx, interaction, "That request no longer exists.").await?;
            return Ok(Some(()));
        };

        if request.decided.is_some() {
            respond(ctx, interaction, "That request was already decided.").await?;
            return Ok(Some(()));
        }

        let section =
            crate::models::find_sponsor_section(state.database.read(), request.discord_user_id)
                .await?;

        if approved {
            let spendable = spendable_cents(state, request.discord_user_id).await?;

            if spendable < request.tier_cents {
                respond(
                    ctx,
                    interaction,
                    &format!(
                        "<@{}> only has `{}` left to spend and this section costs `{}`.",
                        request.discord_user_id,
                        amount(spendable, &state.env.sponsors_currency),
                        amount(request.tier_cents, &state.env.sponsors_currency)
                    ),
                )
                .await?;
                return Ok(Some(()));
            }
        }

        if !crate::models::decide_sponsor_section_request(state.database.write(), id, approved)
            .await?
        {
            respond(ctx, interaction, "That request was already decided.").await?;
            return Ok(Some(()));
        }

        if approved && !crate::sponsors::start_section(state, &request).await? {
            crate::models::reopen_sponsor_section_request(state.database.write(), id).await?;

            respond(
                ctx,
                interaction,
                "Their funds ran out while this was being approved, try again later.",
            )
            .await?;
            return Ok(Some(()));
        }

        if !approved
            && request.logo_path
                != section
                    .as_ref()
                    .and_then(|section| section.logo_path.clone())
        {
            crate::sponsors::remove_logo(state, request.logo_path.as_deref()).await;
        }

        crate::sponsors::close_request(state, &request, section.as_ref(), interaction, approved)
            .await?;

        tracing::info!(
            user = %interaction.user.name,
            request = id,
            approved,
            "sponsor section request decided"
        );

        Ok(Some(()))
    }
}

pub struct SponsorSectionRevoke;

#[async_trait::async_trait]
impl crate::components::Component for SponsorSectionRevoke {
    async fn execute(
        &self,
        state: &crate::routes::State,
        ctx: &serenity::prelude::Context,
        interaction: &ComponentInteraction,
    ) -> Result<Option<()>, anyhow::Error> {
        if !matches!(interaction.data.kind, ComponentInteractionDataKind::Button) {
            return Ok(None);
        }

        let Some(discord_user_id) = interaction
            .data
            .custom_id
            .strip_prefix(super::SPONSOR_SECTION_REVOKE_ID)
            .and_then(|discord_user_id| discord_user_id.parse::<i64>().ok())
        else {
            return Ok(None);
        };

        if !is_moderator(state, interaction.channel_id) {
            respond(
                ctx,
                interaction,
                "Sponsor sections are only revoked in the moderator channel.",
            )
            .await?;
            return Ok(Some(()));
        }

        let section =
            crate::models::find_sponsor_section(state.database.read(), discord_user_id).await?;

        if !section.is_some_and(|section| section.active) {
            respond(ctx, interaction, "That section is not on the website.").await?;
            return Ok(Some(()));
        }

        interaction
            .create_response(
                &ctx.http,
                SponsorSectionRevokeModal::create(
                    None,
                    format!(
                        "{}{discord_user_id}",
                        super::SPONSOR_SECTION_REVOKE_MODAL_ID
                    ),
                ),
            )
            .await?;

        Ok(Some(()))
    }
}

pub async fn revoke(
    state: &crate::routes::State,
    ctx: &serenity::prelude::Context,
    interaction: &ModalInteraction,
) -> Result<Option<()>, anyhow::Error> {
    let Some(discord_user_id) = interaction
        .data
        .custom_id
        .strip_prefix(super::SPONSOR_SECTION_REVOKE_MODAL_ID)
        .and_then(|discord_user_id| discord_user_id.parse::<i64>().ok())
    else {
        return Ok(None);
    };

    let modal = SponsorSectionRevokeModal::parse(interaction.data.clone());

    let reply = async |content: &str| -> Result<(), anyhow::Error> {
        interaction
            .create_response(
                &ctx.http,
                CreateInteractionResponse::Message(
                    CreateInteractionResponseMessage::new()
                        .content(content.to_string())
                        .ephemeral(true),
                ),
            )
            .await?;

        Ok(())
    };

    if !modal.confirm {
        reply("The section was left on the website.").await?;
        return Ok(Some(()));
    }

    if !is_moderator(state, interaction.channel_id) {
        reply("Sponsor sections are only revoked in the moderator channel.").await?;
        return Ok(Some(()));
    }

    let section =
        crate::models::find_sponsor_section(state.database.read(), discord_user_id).await?;

    let Some(section) = section.filter(|section| section.active) else {
        reply("That section is not on the website.").await?;
        return Ok(Some(()));
    };

    let refunded = crate::sponsors::end_section(state, &section).await?;

    interaction
        .create_response(
            &ctx.http,
            CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new()
                    .components(vec![crate::sponsors::reinstate_button(discord_user_id)]),
            ),
        )
        .await?;

    crate::sponsors::alert(
        state,
        "Sponsor section revoked",
        format!(
            "<@{discord_user_id}>'s section (**{}**) was taken off the website by <@{}>. `{}` went back to their funds.",
            section.name,
            interaction.user.id.get(),
            amount(refunded, &state.env.sponsors_currency)
        ),
    )
    .await?;

    tracing::info!(
        user = %interaction.user.name,
        discord_user_id,
        refunded,
        "sponsor section revoked"
    );

    Ok(Some(()))
}

pub struct SponsorSectionReinstate;

#[async_trait::async_trait]
impl crate::components::Component for SponsorSectionReinstate {
    async fn execute(
        &self,
        state: &crate::routes::State,
        ctx: &serenity::prelude::Context,
        interaction: &ComponentInteraction,
    ) -> Result<Option<()>, anyhow::Error> {
        if !matches!(interaction.data.kind, ComponentInteractionDataKind::Button) {
            return Ok(None);
        }

        let Some(discord_user_id) = interaction
            .data
            .custom_id
            .strip_prefix(super::SPONSOR_SECTION_REINSTATE_ID)
            .and_then(|discord_user_id| discord_user_id.parse::<i64>().ok())
        else {
            return Ok(None);
        };

        if !is_moderator(state, interaction.channel_id) {
            respond(
                ctx,
                interaction,
                "Sponsor sections are only reinstated in the moderator channel.",
            )
            .await?;
            return Ok(Some(()));
        }

        let Some(section) =
            crate::models::find_sponsor_section(state.database.read(), discord_user_id).await?
        else {
            respond(ctx, interaction, "That section no longer exists.").await?;
            return Ok(Some(()));
        };

        if section.active {
            respond(ctx, interaction, "That section is already on the website.").await?;
            return Ok(Some(()));
        }

        if !crate::sponsors::reinstate_section(state, &section).await? {
            respond(
                ctx,
                interaction,
                &format!(
                    "<@{discord_user_id}> cannot cover the `{}` this section costs.",
                    amount(section.tier_cents, &state.env.sponsors_currency)
                ),
            )
            .await?;
            return Ok(Some(()));
        }

        interaction
            .create_response(
                &ctx.http,
                CreateInteractionResponse::UpdateMessage(
                    CreateInteractionResponseMessage::new()
                        .components(vec![crate::sponsors::revoke_button(discord_user_id)]),
                ),
            )
            .await?;

        crate::sponsors::alert(
            state,
            "Sponsor section reinstated",
            format!(
                "<@{discord_user_id}>'s section (**{}**) was put back on the website by <@{}> for `{}`.",
                section.name,
                interaction.user.id.get(),
                amount(section.tier_cents, &state.env.sponsors_currency)
            ),
        )
        .await?;

        tracing::info!(
            user = %interaction.user.name,
            discord_user_id,
            "sponsor section reinstated"
        );

        Ok(Some(()))
    }
}
