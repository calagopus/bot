use sqlx::{FromRow, Row, sqlite::SqliteRow};

#[derive(Debug)]
pub struct SentSponsorship {
    pub id: String,
    pub account_id: Option<i64>,
    pub amount: Option<i64>,
    /// Currency `amount` and `paid` are in, `None` for rows written in the ledger currency
    /// before it was recorded.
    pub currency: Option<String>,
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
            account_id: row.try_get("account_id")?,
            amount: row.try_get("amount")?,
            currency: row.try_get("currency")?,
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

pub async fn sponsorships_without_message(
    pool: &sqlx::SqlitePool,
    provider: crate::sponsors::Provider,
) -> Result<Vec<SentSponsorship>, anyhow::Error> {
    let sponsorships = sqlx::query_as::<_, SentSponsorship>(
        "SELECT * FROM sent_sponsorships WHERE provider = ? AND message_id IS NULL ORDER BY created ASC",
    )
    .bind(provider.key())
    .fetch_all(pool)
    .await?;

    Ok(sponsorships)
}

pub async fn linked_message_ids(pool: &sqlx::SqlitePool) -> Result<Vec<i64>, anyhow::Error> {
    let message_ids: Vec<i64> =
        sqlx::query_scalar("SELECT message_id FROM sent_sponsorships WHERE message_id IS NOT NULL")
            .fetch_all(pool)
            .await?;

    Ok(message_ids)
}

#[derive(Debug)]
pub struct NewSentSponsorship<'a> {
    pub id: &'a str,
    pub provider: crate::sponsors::Provider,
    pub account_id: Option<i64>,
    pub amount: Option<i64>,
    pub currency: &'a str,
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
        "INSERT INTO sent_sponsorships (id, provider, account_id, amount, currency, message_id, recurring, paid, created) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT (id) DO NOTHING",
    )
    .bind(sponsorship.id)
    .bind(sponsorship.provider.key())
    .bind(sponsorship.account_id)
    .bind(sponsorship.amount)
    .bind(sponsorship.currency)
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
    account_id: Option<i64>,
    amount: Option<i64>,
    currency: &str,
    recurring: bool,
) -> Result<(), anyhow::Error> {
    sqlx::query(
        "UPDATE sent_sponsorships SET account_id = ?, amount = ?, currency = ?, recurring = ? WHERE id = ?",
    )
    .bind(account_id)
    .bind(amount)
    .bind(currency)
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
    amount: i64,
    paid: i64,
    currency: &str,
    ended: bool,
) -> Result<(), anyhow::Error> {
    sqlx::query(
        "UPDATE sent_sponsorships SET amount = ?, paid = ?, currency = ?, ended = ?, recurring = TRUE WHERE id = ?",
    )
    .bind(amount)
    .bind(paid)
    .bind(currency)
    .bind(ended)
        .bind(id)
        .execute(pool)
        .await?;

    Ok(())
}
