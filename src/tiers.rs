use serenity::all::{GuildId, RoleId, UserId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TierKind {
    Donator,
    Sponsor,
}

impl TierKind {
    #[inline]
    fn label(self) -> &'static str {
        match self {
            Self::Donator => "Donator",
            Self::Sponsor => "Sponsor",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Tier {
    pub name: String,
    pub entry_in_cents: i64,
    pub role_id: u64,
}

fn roman(mut value: usize) -> String {
    const NUMERALS: [(usize, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];

    let mut roman = String::new();
    for (amount, numeral) in NUMERALS {
        while value >= amount {
            roman.push_str(numeral);
            value -= amount;
        }
    }

    roman
}

/// Entries are `<entry amount in cents>:<discord role id>`, lowest entry amount first.
/// The lowest tier is tier 0, which every sponsor above zero qualifies for.
pub fn parse(kind: TierKind, raw: &str) -> Vec<Tier> {
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
        .enumerate()
        .map(|(index, (entry_in_cents, role_id))| Tier {
            name: match index {
                0 => format!("{} Tier 0", kind.label()),
                index => format!("{} Tier {}", kind.label(), roman(index)),
            },
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

pub fn managed_role_ids(env: &crate::env::Env) -> Vec<RoleId> {
    let mut managed: Vec<RoleId> = Vec::new();

    for tier in env.donator_tiers.iter().chain(&env.sponsor_tiers) {
        let role_id = RoleId::new(tier.role_id);

        if !managed.contains(&role_id) {
            managed.push(role_id);
        }
    }

    managed
}

pub fn earned_role_ids(
    env: &crate::env::Env,
    evaluation: &crate::sponsors::Evaluation,
    github_id: i64,
) -> Vec<RoleId> {
    let Some(sponsor) = evaluation
        .sponsors
        .iter()
        .find(|sponsor| sponsor.github_id == Some(github_id))
    else {
        return Vec::new();
    };

    [
        resolve(&env.donator_tiers, sponsor.lifetime_in_cents),
        resolve(&env.sponsor_tiers, sponsor.monthly_in_cents),
    ]
    .into_iter()
    .flatten()
    .map(|tier| RoleId::new(tier.role_id))
    .collect()
}

/// Brings the member's tier roles in line with `wanted`, leaving every unmanaged role alone.
pub async fn apply_roles(
    state: &crate::routes::InnerState,
    guild_id: GuildId,
    user_id: UserId,
    wanted: &[RoleId],
) -> Result<(), anyhow::Error> {
    let managed = managed_role_ids(&state.env);
    if managed.is_empty() {
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
    github_id: i64,
    discord_user_id: u64,
) -> Result<(), anyhow::Error> {
    if managed_role_ids(&state.env).is_empty() {
        return Ok(());
    }

    let ledger = crate::sponsors::cached_ledger(&state.env).await?;
    let evaluation = crate::sponsors::evaluate(&ledger, chrono::Utc::now());
    let guild_id = crate::utils::guild_id(state).await?;

    apply_roles(
        state,
        guild_id,
        UserId::new(discord_user_id),
        &earned_role_ids(&state.env, &evaluation, github_id),
    )
    .await
}
