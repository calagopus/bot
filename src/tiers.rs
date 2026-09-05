use serenity::all::{GuildId, RoleId, UserId};

#[derive(Debug, Clone)]
pub struct Tier {
    pub entry_in_cents: i64,
    pub role_id: u64,
}

/// Entries are `<entry amount in cents>:<discord role id>`, lowest entry amount first.
/// The lowest entry qualifies every sponsor above zero.
pub fn parse(raw: &str) -> Vec<Tier> {
    let mut entries: Vec<(i64, u64)> = raw
        .split(',')
        .filter_map(|entry| {
            let (entry_in_cents, role_id) = entry.trim().split_once(':')?;

            Some((
                entry_in_cents.trim().parse().ok()?,
                role_id.trim().parse().ok()?,
            ))
        })
        .collect();

    entries.sort_by_key(|(entry_in_cents, _)| *entry_in_cents);

    entries
        .into_iter()
        .map(|(entry_in_cents, role_id)| Tier {
            entry_in_cents,
            role_id,
        })
        .collect()
}

pub fn resolve(tiers: &[Tier], amount_in_cents: i64) -> Option<&Tier> {
    if amount_in_cents <= 0 {
        return None;
    }

    tiers
        .iter()
        .rev()
        .find(|tier| amount_in_cents >= tier.entry_in_cents)
}

/// Roles granted on the first qualifying sponsorship and never taken away again.
pub fn base_role_ids(env: &crate::env::Env) -> Vec<RoleId> {
    let mut base: Vec<RoleId> = Vec::new();

    for role_id in [env.donator_base_role_id, env.sponsor_base_role_id]
        .into_iter()
        .flatten()
    {
        let role_id = RoleId::new(role_id);

        if !base.contains(&role_id) {
            base.push(role_id);
        }
    }

    base
}

/// Tier roles the bot adds and removes as sponsorships change.
///
/// Base roles are left out even when they also appear in a tier list, so an always-keep
/// role can never be stripped by a misconfiguration.
pub fn managed_role_ids(env: &crate::env::Env) -> Vec<RoleId> {
    let base = base_role_ids(env);
    let mut managed: Vec<RoleId> = Vec::new();

    for tier in env.donator_tiers.iter().chain(&env.sponsor_tiers) {
        let role_id = RoleId::new(tier.role_id);

        if !base.contains(&role_id) && !managed.contains(&role_id) {
            managed.push(role_id);
        }
    }

    managed
}

/// Whether any sponsorship role is configured at all, base or tier.
#[inline]
pub fn is_configured(env: &crate::env::Env) -> bool {
    !managed_role_ids(env).is_empty() || !base_role_ids(env).is_empty()
}

pub fn earned_role_ids(
    env: &crate::env::Env,
    evaluation: &crate::sponsors::Evaluation,
    accounts: &[(crate::sponsors::Provider, String)],
) -> Vec<RoleId> {
    let (lifetime_in_cents, monthly_in_cents) = evaluation.totals(accounts);

    let mut earned: Vec<RoleId> = Vec::new();

    // A base role is earned by any amount above zero, the same threshold `resolve` uses,
    // so it does not depend on the matching tier list being configured.
    for (base_role_id, amount_in_cents) in [
        (env.donator_base_role_id, lifetime_in_cents),
        (env.sponsor_base_role_id, monthly_in_cents),
    ] {
        if amount_in_cents <= 0 {
            continue;
        }

        if let Some(role_id) = base_role_id {
            let role_id = RoleId::new(role_id);

            if !earned.contains(&role_id) {
                earned.push(role_id);
            }
        }
    }

    for tier in [
        resolve(&env.donator_tiers, lifetime_in_cents),
        resolve(&env.sponsor_tiers, monthly_in_cents),
    ]
    .into_iter()
    .flatten()
    {
        let role_id = RoleId::new(tier.role_id);

        if !earned.contains(&role_id) {
            earned.push(role_id);
        }
    }

    earned
}

/// Brings the member's tier roles in line with `wanted`, leaving every unmanaged role alone.
///
/// Base roles are only ever granted: once a member has one it stays, even after the
/// sponsorship lapses or their accounts are unlinked.
pub async fn apply_roles(
    state: &crate::routes::InnerState,
    guild_id: GuildId,
    user_id: UserId,
    wanted: &[RoleId],
) -> Result<(), anyhow::Error> {
    let managed = managed_role_ids(&state.env);
    let base = base_role_ids(&state.env);
    if managed.is_empty() && base.is_empty() {
        return Ok(());
    }

    let http = state.bot.read().await.clone();

    let member = match http.get_member(guild_id, user_id).await {
        Ok(member) => member,
        Err(err) => {
            tracing::debug!(
                "skipping sponsorship tier roles for discord user {}: {:?}",
                user_id,
                err
            );
            return Ok(());
        }
    };

    for role_id in &base {
        if !wanted.contains(role_id) || member.roles.contains(role_id) {
            continue;
        }

        match http
            .add_member_role(guild_id, user_id, *role_id, Some("sponsorship base role"))
            .await
        {
            Ok(()) => tracing::info!(
                "added sponsorship base role {} to discord user {}",
                role_id,
                user_id
            ),
            Err(err) => tracing::error!(
                "failed to add sponsorship base role {} for discord user {}: {:?}",
                role_id,
                user_id,
                err
            ),
        }
    }

    for role_id in &managed {
        let has = member.roles.contains(role_id);
        let wants = wanted.contains(role_id);

        let result = if wants && !has {
            http.add_member_role(guild_id, user_id, *role_id, Some("sponsorship tier"))
                .await
        } else if has && !wants {
            http.remove_member_role(guild_id, user_id, *role_id, Some("sponsorship tier"))
                .await
        } else {
            continue;
        };

        match result {
            Ok(()) if wants => tracing::info!(
                "added sponsorship tier role {} to discord user {}",
                role_id,
                user_id
            ),
            Ok(()) => tracing::info!(
                "removed sponsorship tier role {} from discord user {}",
                role_id,
                user_id
            ),
            Err(err) => tracing::error!(
                "failed to update sponsorship tier role {} for discord user {}: {:?}",
                role_id,
                user_id,
                err
            ),
        }
    }

    Ok(())
}

pub async fn sync_linked_member(
    state: &crate::routes::InnerState,
    discord_user_id: u64,
) -> Result<(), anyhow::Error> {
    if !is_configured(&state.env) {
        return Ok(());
    }

    let accounts = crate::models::find_linked_accounts_by_discord(
        state.database.read(),
        discord_user_id as i64,
    )
    .await?
    .into_iter()
    .filter_map(|account| Some((account.provider()?, account.provider_id)))
    .collect::<Vec<_>>();

    let guild_id = crate::utils::guild_id(state).await?;
    let wanted = if accounts.is_empty() {
        Vec::new()
    } else {
        let ledger = crate::sponsors::cached_ledger(&state.env).await?;
        if !ledger.is_complete_for(&accounts) {
            return Ok(());
        }

        let evaluation = crate::sponsors::evaluate(&ledger);

        earned_role_ids(&state.env, &evaluation, &accounts)
    };

    apply_roles(state, guild_id, UserId::new(discord_user_id), &wanted).await
}
