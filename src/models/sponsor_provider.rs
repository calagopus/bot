/// Tracks empty first polls too, so the first later contribution is announced.
pub async fn is_provider_seeded(
    pool: &sqlx::SqlitePool,
    provider: crate::sponsors::Provider,
) -> Result<bool, anyhow::Error> {
    let seeded: Option<i64> =
        sqlx::query_scalar("SELECT 1 FROM sponsor_providers WHERE provider = ?")
            .bind(provider.key())
            .fetch_optional(pool)
            .await?;

    Ok(seeded.is_some())
}

pub async fn mark_provider_seeded(
    pool: &sqlx::SqlitePool,
    provider: crate::sponsors::Provider,
) -> Result<(), anyhow::Error> {
    sqlx::query(
        "INSERT INTO sponsor_providers (provider) VALUES (?)
         ON CONFLICT (provider) DO NOTHING",
    )
    .bind(provider.key())
    .execute(pool)
    .await?;

    Ok(())
}
