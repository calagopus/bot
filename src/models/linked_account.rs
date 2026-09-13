use sqlx::{FromRow, Row, sqlite::SqliteRow};

#[derive(Debug, Clone)]
pub struct LinkedAccount {
    pub provider: String,
    pub provider_id: String,
    pub login: Option<String>,
    pub discord_user_id: Option<i64>,

    /// High-water mark of what the ledger reported for this account, already credited to
    /// whoever it was linked to at the time. Held per account so relinking moves the credit
    /// with it instead of paying it out twice.
    pub lifetime_cents: i64,
    /// How much of a drop below [`LinkedAccount::lifetime_cents`] moderators were told about.
    pub refunded_cents: i64,
}

impl LinkedAccount {
    #[inline]
    pub fn provider(&self) -> Option<crate::sponsors::Provider> {
        crate::sponsors::Provider::from_key(&self.provider)
    }
}

impl FromRow<'_, SqliteRow> for LinkedAccount {
    fn from_row(row: &SqliteRow) -> sqlx::Result<Self> {
        Ok(Self {
            provider: row.try_get("provider")?,
            provider_id: row.try_get("provider_id")?,
            login: row.try_get("login")?,
            discord_user_id: row.try_get("discord_user_id")?,
            lifetime_cents: row.try_get("lifetime_cents")?,
            refunded_cents: row.try_get("refunded_cents")?,
        })
    }
}

pub async fn find_linked_accounts_by_discord(
    pool: &sqlx::SqlitePool,
    discord_user_id: i64,
) -> Result<Vec<LinkedAccount>, anyhow::Error> {
    let accounts = sqlx::query_as::<_, LinkedAccount>(
        "SELECT * FROM linked_accounts WHERE discord_user_id = ? ORDER BY provider ASC, id ASC",
    )
    .bind(discord_user_id)
    .fetch_all(pool)
    .await?;

    Ok(accounts)
}

pub async fn linked_accounts(pool: &sqlx::SqlitePool) -> Result<Vec<LinkedAccount>, anyhow::Error> {
    let accounts = sqlx::query_as::<_, LinkedAccount>(
        "SELECT * FROM linked_accounts WHERE discord_user_id IS NOT NULL ORDER BY discord_user_id ASC",
    )
    .fetch_all(pool)
    .await?;

    Ok(accounts)
}

pub async fn upsert_linked_account(
    pool: &sqlx::SqlitePool,
    provider: crate::sponsors::Provider,
    provider_id: &str,
    login: Option<&str>,
) -> Result<i64, anyhow::Error> {
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO linked_accounts (provider, provider_id, login) VALUES (?, ?, ?)
         ON CONFLICT (provider, provider_id) DO UPDATE SET login = COALESCE(excluded.login, linked_accounts.login)
         RETURNING id",
    )
    .bind(provider.key())
    .bind(provider_id)
    .bind(login)
    .fetch_one(pool)
    .await?;

    Ok(id)
}

/// Replaces this member's provider links and returns displaced Discord user IDs for role sync.
pub async fn link_accounts(
    pool: &sqlx::SqlitePool,
    provider: crate::sponsors::Provider,
    accounts: &[(String, String)],
    discord_user_id: i64,
) -> Result<Vec<i64>, anyhow::Error> {
    let mut transaction = pool.begin().await?;

    let mut displaced_user_ids: Vec<i64> = Vec::new();

    for (provider_id, _) in accounts {
        let owner_id: Option<i64> = sqlx::query_scalar(
            "SELECT discord_user_id FROM linked_accounts WHERE provider = ? AND provider_id = ?",
        )
        .bind(provider.key())
        .bind(provider_id)
        .fetch_optional(&mut *transaction)
        .await?
        .flatten();

        if let Some(owner_id) = owner_id
            && owner_id != discord_user_id
            && !displaced_user_ids.contains(&owner_id)
        {
            displaced_user_ids.push(owner_id);
        }
    }

    sqlx::query(
        "UPDATE linked_accounts SET discord_user_id = NULL WHERE provider = ? AND discord_user_id = ?",
    )
    .bind(provider.key())
    .bind(discord_user_id)
    .execute(&mut *transaction)
    .await?;

    for (provider_id, login) in accounts {
        sqlx::query(
            "INSERT INTO linked_accounts (provider, provider_id, login, discord_user_id) VALUES (?, ?, ?, ?)
             ON CONFLICT (provider, provider_id) DO UPDATE SET login = excluded.login, discord_user_id = excluded.discord_user_id",
        )
        .bind(provider.key())
        .bind(provider_id)
        .bind(login)
        .bind(discord_user_id)
        .execute(&mut *transaction)
        .await?;
    }

    transaction.commit().await?;

    Ok(displaced_user_ids)
}

pub async fn unlink_discord_user(
    pool: &sqlx::SqlitePool,
    provider: crate::sponsors::Provider,
    discord_user_id: i64,
) -> Result<Vec<LinkedAccount>, anyhow::Error> {
    let accounts = sqlx::query_as::<_, LinkedAccount>(
        "UPDATE linked_accounts SET discord_user_id = NULL WHERE provider = ? AND discord_user_id = ? RETURNING *",
    )
    .bind(provider.key())
    .bind(discord_user_id)
    .fetch_all(pool)
    .await?;

    Ok(accounts)
}

/// Moves the account's high-water mark up to what the ledger now reports. The difference is
/// what the member has gained to spend since the last poll.
pub async fn credit_linked_account(
    pool: &sqlx::SqlitePool,
    provider: crate::sponsors::Provider,
    provider_id: &str,
    lifetime_cents: i64,
) -> Result<(), anyhow::Error> {
    sqlx::query(
        "UPDATE linked_accounts SET lifetime_cents = ? WHERE provider = ? AND provider_id = ?",
    )
    .bind(lifetime_cents)
    .bind(provider.key())
    .bind(provider_id)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn set_linked_account_refunded(
    pool: &sqlx::SqlitePool,
    provider: crate::sponsors::Provider,
    provider_id: &str,
    refunded_cents: i64,
) -> Result<(), anyhow::Error> {
    sqlx::query(
        "UPDATE linked_accounts SET refunded_cents = ? WHERE provider = ? AND provider_id = ?",
    )
    .bind(refunded_cents)
    .bind(provider.key())
    .bind(provider_id)
    .execute(pool)
    .await?;

    Ok(())
}
