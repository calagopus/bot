pub mod sponsor_section;
pub mod text_message;
pub mod text_message_modify;

/// Modals opened from a button rather than from a command come back as their own interaction,
/// which the dispatcher below routes by the custom ID the button set.
pub async fn execute_modal(
    state: &crate::routes::State,
    ctx: &serenity::prelude::Context,
    interaction: &serenity::all::ModalInteraction,
) -> Result<Option<()>, anyhow::Error> {
    let custom_id = interaction.data.custom_id.as_str();

    if custom_id.starts_with(crate::components::SPONSOR_SECTION_MODAL_ID) {
        return crate::components::sponsor_section::submit(state, ctx, interaction).await;
    }

    if custom_id.starts_with(crate::components::SPONSOR_SECTION_REVOKE_MODAL_ID) {
        return crate::components::sponsor_section::revoke(state, ctx, interaction).await;
    }

    Ok(None)
}
