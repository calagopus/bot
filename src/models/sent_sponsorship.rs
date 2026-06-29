use sqlx::{FromRow, Row, sqlite::SqliteRow};

#[derive(Debug)]
pub struct SentSponsorship {
    pub id: String,
    pub github_id: Option<i64>,
    pub amount: Option<i64>,
    pub created: Option<chrono::DateTime<chrono::Utc>>,
}

impl FromRow<'_, SqliteRow> for SentSponsorship {
    fn from_row(row: &SqliteRow) -> sqlx::Result<Self> {
        Ok(Self {
            id: row.try_get("id")?,
            github_id: row.try_get("github_id")?,
            amount: row.try_get("amount")?,
            created: row
                .try_get::<Option<i64>, _>("created")?
                .and_then(|c| chrono::DateTime::<chrono::Utc>::from_timestamp(c, 0)),
        })
    }
}

pub async fn find_sent_sponsorship(
    pool: &sqlx::SqlitePool,
    id: &str,
) -> Result<SentSponsorship, anyhow::Error> {
    let sponsorship =
        sqlx::query_as::<_, SentSponsorship>("SELECT * FROM sent_sponsorships WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await?;

    sponsorship.ok_or_else(|| anyhow::anyhow!("Sent sponsorship not found"))
}

pub async fn insert_sent_sponsorship(
    pool: &sqlx::SqlitePool,
    id: &str,
    github_id: Option<i64>,
    amount: Option<i64>,
    created: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<(), anyhow::Error> {
    sqlx::query(
        "INSERT INTO sent_sponsorships (id, github_id, amount, created) VALUES (?, ?, ?, ?)",
    )
    .bind(id)
    .bind(github_id)
    .bind(amount)
    .bind(
        created
            .map(|c| c.timestamp())
            .unwrap_or_else(|| chrono::Utc::now().timestamp()),
    )
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn backfill_sent_sponsorship(
    pool: &sqlx::SqlitePool,
    id: &str,
    github_id: Option<i64>,
    amount: Option<i64>,
) -> Result<(), anyhow::Error> {
    sqlx::query("UPDATE sent_sponsorships SET github_id = ?, amount = ? WHERE id = ?")
        .bind(github_id)
        .bind(amount)
        .bind(id)
        .execute(pool)
        .await?;

    Ok(())
}
