use sqlx::{FromRow, Row, sqlite::SqliteRow};

#[derive(Debug)]
pub struct SentSponsorship {
    pub id: String,
    pub created: Option<chrono::DateTime<chrono::Utc>>,
}

impl FromRow<'_, SqliteRow> for SentSponsorship {
    fn from_row(row: &SqliteRow) -> sqlx::Result<Self> {
        Ok(Self {
            id: row.try_get("id")?,
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
    created: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<(), anyhow::Error> {
    sqlx::query("INSERT INTO sent_sponsorships (id, created) VALUES (?, ?)")
        .bind(id)
        .bind(
            created
                .map(|c| c.timestamp())
                .unwrap_or_else(|| chrono::Utc::now().timestamp()),
        )
        .execute(pool)
        .await?;

    Ok(())
}
