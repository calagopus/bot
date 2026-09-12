use std::sync::{LazyLock, OnceLock};

mod agent;
mod answer;
mod guard;
mod mcp;
mod openrouter;
mod prompt;
mod render;
mod reply;
mod support;
mod transcript;

pub use reply::spawn_enabled_answer;
pub use support::{handle_message_create, handle_thread_create};

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .user_agent(format!("Calagopus Bot ({})", crate::VERSION))
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .expect("failed to build ai http client")
});

static MCP_CLIENT: OnceLock<mcp::Client> = OnceLock::new();

#[inline]
fn mcp_client(env: &crate::env::Env) -> &'static mcp::Client {
    MCP_CLIENT.get_or_init(|| mcp::Client::new(env.ai_support_mcp_url.clone()))
}

#[inline]
fn docs_url(env: &crate::env::Env) -> &str {
    env.ai_support_mcp_url
        .trim_end_matches('/')
        .trim_end_matches("/mcp")
}

fn extension(filename: &str) -> Option<String> {
    filename
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_lowercase())
}

#[inline]
fn is_configured(env: &crate::env::Env) -> bool {
    env.ai_support_channel_id.is_some() && env.openrouter_api_key.is_some()
}

#[inline]
pub fn is_staff(env: &crate::env::Env, roles: &[serenity::all::RoleId]) -> bool {
    env.ai_support_role_ids
        .iter()
        .any(|role_id| roles.contains(&serenity::all::RoleId::new(*role_id)))
}
