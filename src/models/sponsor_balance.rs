pub async fn available_cents(
    pool: &sqlx::SqlitePool,
    discord_user_id: i64,
) -> Result<i64, anyhow::Error> {
    let available: Option<i64> = sqlx::query_scalar(
        "SELECT available_cents FROM sponsor_balances WHERE discord_user_id = ?",
    )
    .bind(discord_user_id)
    .fetch_optional(pool)
    .await?;

    Ok(available.unwrap_or(0))
}

/// Adds to what a member can spend; a negative amount takes funds away. Returns the new
/// balance, which the caller is expected to have checked is not going negative.
pub async fn add_available_cents(
    pool: &sqlx::SqlitePool,
    discord_user_id: i64,
    cents: i64,
) -> Result<i64, anyhow::Error> {
    let available: i64 = sqlx::query_scalar(
        "INSERT INTO sponsor_balances (discord_user_id, available_cents) VALUES (?, MAX(?, 0))
         ON CONFLICT (discord_user_id) DO UPDATE SET available_cents = MAX(sponsor_balances.available_cents + ?, 0)
         RETURNING available_cents",
    )
    .bind(discord_user_id)
    .bind(cents)
    .bind(cents)
    .fetch_one(pool)
    .await?;

    Ok(available)
}

/// Takes the amount only when the member can cover it, so two interactions cannot spend the
/// same funds twice.
pub async fn take_available_cents(
    pool: &sqlx::SqlitePool,
    discord_user_id: i64,
    cents: i64,
) -> Result<bool, anyhow::Error> {
    let taken = sqlx::query(
        "UPDATE sponsor_balances SET available_cents = available_cents - ?
         WHERE discord_user_id = ? AND available_cents >= ?",
    )
    .bind(cents)
    .bind(discord_user_id)
    .bind(cents)
    .execute(pool)
    .await?;

    Ok(taken.rows_affected() > 0)
}
