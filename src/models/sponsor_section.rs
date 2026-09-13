use sqlx::{FromRow, Row, sqlite::SqliteRow};

#[inline]
fn timestamp(seconds: i64) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp(seconds, 0).unwrap_or_default()
}

/// What a sponsor bought, and what the website shows for them.
#[derive(Debug, Clone)]
pub struct SponsorSection {
    pub discord_user_id: i64,
    pub tier_cents: i64,

    pub name: String,
    pub description: Option<String>,
    pub logo_path: Option<String>,
    pub link: Option<String>,

    pub active: bool,
    pub renewal_warned: bool,

    pub period_start: chrono::DateTime<chrono::Utc>,
    pub period_end: chrono::DateTime<chrono::Utc>,
}

impl FromRow<'_, SqliteRow> for SponsorSection {
    fn from_row(row: &SqliteRow) -> sqlx::Result<Self> {
        Ok(Self {
            discord_user_id: row.try_get("discord_user_id")?,
            tier_cents: row.try_get("tier_cents")?,
            name: row.try_get("name")?,
            description: row.try_get("description")?,
            logo_path: row.try_get("logo_path")?,
            link: row.try_get("link")?,
            active: row.try_get("active")?,
            renewal_warned: row.try_get("renewal_warned")?,
            period_start: timestamp(row.try_get("period_start")?),
            period_end: timestamp(row.try_get("period_end")?),
        })
    }
}

/// A submitted section that a moderator has not decided on yet, or the record of one they have.
#[derive(Debug, Clone)]
pub struct SponsorSectionRequest {
    pub id: i64,
    pub discord_user_id: i64,
    pub tier_cents: i64,

    pub name: String,
    pub description: Option<String>,
    pub logo_path: Option<String>,
    pub link: Option<String>,

    /// Set once a moderator decides; the outcome itself is kept in the row for the record.
    pub decided: Option<chrono::DateTime<chrono::Utc>>,
}

impl FromRow<'_, SqliteRow> for SponsorSectionRequest {
    fn from_row(row: &SqliteRow) -> sqlx::Result<Self> {
        Ok(Self {
            id: row.try_get("id")?,
            discord_user_id: row.try_get("discord_user_id")?,
            tier_cents: row.try_get("tier_cents")?,
            name: row.try_get("name")?,
            description: row.try_get("description")?,
            logo_path: row.try_get("logo_path")?,
            link: row.try_get("link")?,
            decided: row.try_get::<Option<i64>, _>("decided")?.map(timestamp),
        })
    }
}

pub async fn find_sponsor_section(
    pool: &sqlx::SqlitePool,
    discord_user_id: i64,
) -> Result<Option<SponsorSection>, anyhow::Error> {
    let section = sqlx::query_as::<_, SponsorSection>(
        "SELECT * FROM sponsor_sections WHERE discord_user_id = ?",
    )
    .bind(discord_user_id)
    .fetch_optional(pool)
    .await?;

    Ok(section)
}

pub async fn active_sponsor_sections(
    pool: &sqlx::SqlitePool,
) -> Result<Vec<SponsorSection>, anyhow::Error> {
    let sections = sqlx::query_as::<_, SponsorSection>(
        "SELECT * FROM sponsor_sections WHERE active = true ORDER BY tier_cents DESC, created ASC",
    )
    .fetch_all(pool)
    .await?;

    Ok(sections)
}

/// Replaces whatever the member had, starting a fresh period at the new tier.
pub async fn upsert_sponsor_section(
    pool: &sqlx::SqlitePool,
    request: &SponsorSectionRequest,
    period_start: chrono::DateTime<chrono::Utc>,
    period_end: chrono::DateTime<chrono::Utc>,
) -> Result<(), anyhow::Error> {
    sqlx::query(
        "INSERT INTO sponsor_sections (discord_user_id, tier_cents, name, description, logo_path, link, active, renewal_warned, period_start, period_end)
         VALUES (?, ?, ?, ?, ?, ?, true, false, ?, ?)
         ON CONFLICT (discord_user_id) DO UPDATE SET
            tier_cents = excluded.tier_cents,
            name = excluded.name,
            description = excluded.description,
            logo_path = excluded.logo_path,
            link = excluded.link,
            active = true,
            renewal_warned = false,
            period_start = excluded.period_start,
            period_end = excluded.period_end",
    )
    .bind(request.discord_user_id)
    .bind(request.tier_cents)
    .bind(&request.name)
    .bind(&request.description)
    .bind(&request.logo_path)
    .bind(&request.link)
    .bind(period_start.timestamp())
    .bind(period_end.timestamp())
    .execute(pool)
    .await?;

    Ok(())
}

/// Ends the current period; the content is kept so it can be bought again later.
pub async fn deactivate_sponsor_section(
    pool: &sqlx::SqlitePool,
    discord_user_id: i64,
) -> Result<(), anyhow::Error> {
    sqlx::query(
        "UPDATE sponsor_sections SET active = false, renewal_warned = false WHERE discord_user_id = ?",
    )
    .bind(discord_user_id)
    .execute(pool)
    .await?;

    Ok(())
}

/// Starts a new paid period, which also puts a section that had stopped back on the website.
pub async fn renew_sponsor_section(
    pool: &sqlx::SqlitePool,
    discord_user_id: i64,
    period_start: chrono::DateTime<chrono::Utc>,
    period_end: chrono::DateTime<chrono::Utc>,
) -> Result<(), anyhow::Error> {
    sqlx::query(
        "UPDATE sponsor_sections SET period_start = ?, period_end = ?, active = true, renewal_warned = false WHERE discord_user_id = ?",
    )
    .bind(period_start.timestamp())
    .bind(period_end.timestamp())
    .bind(discord_user_id)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn mark_sponsor_section_warned(
    pool: &sqlx::SqlitePool,
    discord_user_id: i64,
) -> Result<(), anyhow::Error> {
    sqlx::query("UPDATE sponsor_sections SET renewal_warned = true WHERE discord_user_id = ?")
        .bind(discord_user_id)
        .execute(pool)
        .await?;

    Ok(())
}

pub async fn find_pending_sponsor_section_request(
    pool: &sqlx::SqlitePool,
    discord_user_id: i64,
) -> Result<Option<SponsorSectionRequest>, anyhow::Error> {
    let request = sqlx::query_as::<_, SponsorSectionRequest>(
        "SELECT * FROM sponsor_section_requests WHERE discord_user_id = ? AND decided IS NULL",
    )
    .bind(discord_user_id)
    .fetch_optional(pool)
    .await?;

    Ok(request)
}

pub async fn find_sponsor_section_request(
    pool: &sqlx::SqlitePool,
    id: i64,
) -> Result<Option<SponsorSectionRequest>, anyhow::Error> {
    let request = sqlx::query_as::<_, SponsorSectionRequest>(
        "SELECT * FROM sponsor_section_requests WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(request)
}

pub struct NewSponsorSectionRequest<'a> {
    pub discord_user_id: i64,
    pub tier_cents: i64,
    pub name: &'a str,
    pub description: Option<&'a str>,
    pub logo_path: Option<&'a str>,
    pub link: Option<&'a str>,
}

/// Fails while the member already has a request waiting on a moderator, which the pending
/// unique index enforces.
pub async fn insert_sponsor_section_request(
    pool: &sqlx::SqlitePool,
    request: NewSponsorSectionRequest<'_>,
) -> Result<SponsorSectionRequest, anyhow::Error> {
    let inserted = sqlx::query_as::<_, SponsorSectionRequest>(
        "INSERT INTO sponsor_section_requests (discord_user_id, tier_cents, name, description, logo_path, link)
         VALUES (?, ?, ?, ?, ?, ?) RETURNING *",
    )
    .bind(request.discord_user_id)
    .bind(request.tier_cents)
    .bind(request.name)
    .bind(request.description)
    .bind(request.logo_path)
    .bind(request.link)
    .fetch_one(pool)
    .await?;

    Ok(inserted)
}

pub async fn set_sponsor_section_request_message(
    pool: &sqlx::SqlitePool,
    id: i64,
    message_id: i64,
) -> Result<(), anyhow::Error> {
    sqlx::query("UPDATE sponsor_section_requests SET message_id = ? WHERE id = ?")
        .bind(message_id)
        .bind(id)
        .execute(pool)
        .await?;

    Ok(())
}

/// Records the moderator's decision, and only for a request nobody has decided on yet, so two
/// moderators pressing at once cannot both go through.
pub async fn decide_sponsor_section_request(
    pool: &sqlx::SqlitePool,
    id: i64,
    approved: bool,
) -> Result<bool, anyhow::Error> {
    let decided = sqlx::query(
        "UPDATE sponsor_section_requests SET approved = ?, decided = ? WHERE id = ? AND decided IS NULL",
    )
    .bind(approved)
    .bind(chrono::Utc::now().timestamp())
    .bind(id)
    .execute(pool)
    .await?;

    Ok(decided.rows_affected() > 0)
}

/// Puts a decided request back in front of the moderators, for when approving it could not go
/// through after all.
pub async fn reopen_sponsor_section_request(
    pool: &sqlx::SqlitePool,
    id: i64,
) -> Result<(), anyhow::Error> {
    sqlx::query(
        "UPDATE sponsor_section_requests SET approved = false, decided = NULL WHERE id = ?",
    )
    .bind(id)
    .execute(pool)
    .await?;

    Ok(())
}
