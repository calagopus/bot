pub mod ai_support;
mod sponsor_unlink;
mod text_message_roles;

pub const SPONSOR_UNLINK_ID: &str = "sponsor_unlink:";
pub const AI_SUPPORT_TOGGLE_ID: &str = "ai_support_toggle:";
pub const AI_SUPPORT_RESOLVE_ID: &str = "ai_support_resolve:";

#[async_trait::async_trait]
pub trait Component {
    async fn execute(
        &self,
        state: &crate::routes::State,
        ctx: &serenity::prelude::Context,
        interaction: &serenity::all::ComponentInteraction,
    ) -> Result<Option<()>, anyhow::Error>;
}

pub struct ComponentList {
    components: Vec<Box<dyn Component + Send + Sync>>,
}

impl ComponentList {
    pub fn new() -> Self {
        Self {
            components: Vec::new(),
        }
    }

    pub fn add_component(mut self, component: impl Component + Send + Sync + 'static) -> Self {
        self.components.push(Box::new(component));
        self
    }

    pub async fn execute_component(
        &self,
        state: &crate::routes::State,
        ctx: &serenity::prelude::Context,
        interaction: &serenity::all::ComponentInteraction,
    ) -> Result<Option<()>, anyhow::Error> {
        for component in &self.components {
            if component.execute(state, ctx, interaction).await?.is_some() {
                return Ok(Some(()));
            }
        }

        Ok(None)
    }
}

pub fn components(list: ComponentList) -> ComponentList {
    list.add_component(text_message_roles::TextMessageRoles)
        .add_component(sponsor_unlink::SponsorUnlink)
        .add_component(ai_support::AiSupportToggle)
        .add_component(ai_support::AiSupportResolve)
}
