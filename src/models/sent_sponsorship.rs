use sqlx::{FromRow, Row, sqlite::SqliteRow};

#[derive(Debug)]
pub struct SentSponsorship {
    pub id: String,
    pub github_id: Option<i64>,
    pub amount: Option<i64>,
    pub message_id: Option<i64>,
    pub recurring: bool,
    pub ended: bool,
    pub paid: Option<i64>,
    pub created: Option<chrono::DateTime<chrono::Utc>>,
}

impl FromRow<'_, SqliteRow> for SentSponsorship {
    fn from_row(row: &SqliteRow) -> sqlx::Result<Self> {
        Ok(Self {
            id: row.try_get("id")?,
            github_id: row.try_get("github_id")?,
            amount: row.try_get("amount")?,
            message_id: row.try_get("message_id")?,
            recurring: row.try_get("recurring")?,
            ended: row.try_get("ended")?,
            paid: row.try_get("paid")?,
            created: row
                .try_get::<Option<i64>, _>("created")?
                .and_then(|c| chrono::DateTime::<chrono::Utc>::from_timestamp(c, 0)),
        })
    }
}

pub async fn find_sent_sponsorship(
    pool: &sqlx::SqlitePool,
    id: &str,
) -> Result<Option<SentSponsorship>, anyhow::Error> {
    let sponsorship =
        sqlx::query_as::<_, SentSponsorship>("SELECT * FROM sent_sponsorships WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await?;

    Ok(sponsorship)
}

pub async fn count_sent_sponsorships(pool: &sqlx::SqlitePool) -> Result<i64, anyhow::Error> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sent_sponsorships")
        .fetch_one(pool)
        .await?;

    Ok(count)
}

pub async fn sponsorships_without_message(
    pool: &sqlx::SqlitePool,
) -> Result<Vec<SentSponsorship>, anyhow::Error> {
    let sponsorships = sqlx::query_as::<_, SentSponsorship>(
        "SELECT * FROM sent_sponsorships WHERE message_id IS NULL ORDER BY created ASC",
    )
    .fetch_all(pool)
    .await?;

    Ok(sponsorships)
}

#[derive(Debug, Default)]
pub struct NewSentSponsorship<'a> {
    pub id: &'a str,
    pub github_id: Option<i64>,
    pub amount: Option<i64>,
    pub message_id: Option<i64>,
    pub recurring: bool,
    pub paid: Option<i64>,
    pub created: Option<chrono::DateTime<chrono::Utc>>,
}

pub async fn insert_sent_sponsorship(
    pool: &sqlx::SqlitePool,
    sponsorship: NewSentSponsorship<'_>,
) -> Result<(), anyhow::Error> {
    sqlx::query(
        "INSERT INTO sent_sponsorships (id, github_id, amount, message_id, recurring, paid, created) VALUES (?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(sponsorship.id)
    .bind(sponsorship.github_id)
    .bind(sponsorship.amount)
    .bind(sponsorship.message_id)
    .bind(sponsorship.recurring)
    .bind(sponsorship.paid)
    .bind(
        sponsorship
            .created
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
    recurring: bool,
) -> Result<(), anyhow::Error> {
    sqlx::query(
        "UPDATE sent_sponsorships SET github_id = ?, amount = ?, recurring = ? WHERE id = ?",
    )
    .bind(github_id)
    .bind(amount)
    .bind(recurring)
    .bind(id)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn set_sent_sponsorship_message(
    pool: &sqlx::SqlitePool,
    id: &str,
    message_id: i64,
) -> Result<(), anyhow::Error> {
    sqlx::query("UPDATE sent_sponsorships SET message_id = ? WHERE id = ?")
        .bind(message_id)
        .bind(id)
        .execute(pool)
        .await?;

    Ok(())
}

pub async fn set_sent_sponsorship_rendered(
    pool: &sqlx::SqlitePool,
    id: &str,
    paid: i64,
    ended: bool,
) -> Result<(), anyhow::Error> {
    sqlx::query("UPDATE sent_sponsorships SET paid = ?, ended = ?, recurring = TRUE WHERE id = ?")
        .bind(paid)
        .bind(ended)
        .bind(id)
        .execute(pool)
        .await?;

    Ok(())
}
