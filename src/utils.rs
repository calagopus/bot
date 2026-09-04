use axum::http::HeaderMap;
use std::net::IpAddr;

#[inline]
pub fn extract_ip(headers: &HeaderMap) -> Option<IpAddr> {
    let ip = headers
        .get("x-real-ip")
        .or_else(|| headers.get("x-forwarded-for"))
        .map(|ip| ip.to_str().unwrap_or_default())
        .unwrap_or_default();

    if ip.is_empty() {
        return None;
    }

    let ip = if ip.contains(',') {
        ip.split(',').next().unwrap_or_default().trim().to_string()
    } else {
        ip.to_string()
    };

    ip.parse().ok()
}

pub async fn guild_id(
    state: &crate::routes::InnerState,
) -> Result<serenity::all::GuildId, anyhow::Error> {
    if let Some(guild_id) = state.env.guild_id {
        return Ok(serenity::all::GuildId::new(guild_id));
    }

    let http = state.bot.read().await.clone();

    let Some(channel) = http
        .get_channel(state.env.github_channel_id.into())
        .await?
        .guild()
    else {
        return Err(anyhow::anyhow!(
            "github channel ID {} is not a guild channel",
            state.env.github_channel_id
        ));
    };

    Ok(channel.base.guild_id)
}
