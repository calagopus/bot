use sqlx::{FromRow, Row, sqlite::SqliteRow};

#[derive(Debug, Clone)]
pub struct AiSupportThread {
    pub thread_id: i64,
    pub message_id: Option<i64>,
    pub owner_id: Option<i64>,

    pub enabled: bool,

    pub paused: bool,
    pub resolved: bool,
}

impl AiSupportThread {
    #[inline]
    pub fn answers(&self) -> bool {
        self.enabled && !self.paused && !self.resolved
    }
}

impl FromRow<'_, SqliteRow> for AiSupportThread {
    fn from_row(row: &SqliteRow) -> sqlx::Result<Self> {
        Ok(Self {
            thread_id: row.try_get("thread_id")?,
            message_id: row.try_get("message_id")?,
            owner_id: row.try_get("owner_id")?,
            enabled: row.try_get("enabled")?,
            paused: row.try_get("paused")?,
            resolved: row.try_get("resolved")?,
        })
    }
}

pub async fn find_ai_support_thread(
    pool: &sqlx::SqlitePool,
    thread_id: i64,
) -> Result<Option<AiSupportThread>, anyhow::Error> {
    let thread = sqlx::query_as::<_, AiSupportThread>(
        "SELECT * FROM ai_support_threads WHERE thread_id = ?",
    )
    .bind(thread_id)
    .fetch_optional(pool)
    .await?;

    Ok(thread)
}

pub async fn insert_ai_support_thread(
    pool: &sqlx::SqlitePool,
    thread_id: i64,
    message_id: i64,
    owner_id: i64,
) -> Result<(), anyhow::Error> {
    sqlx::query(
        "INSERT INTO ai_support_threads (thread_id, message_id, owner_id) VALUES (?, ?, ?)
         ON CONFLICT (thread_id) DO NOTHING",
    )
    .bind(thread_id)
    .bind(message_id)
    .bind(owner_id)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn set_ai_support_enabled(
    pool: &sqlx::SqlitePool,
    thread_id: i64,
    enabled: bool,
) -> Result<(), anyhow::Error> {
    sqlx::query("UPDATE ai_support_threads SET enabled = ?, paused = 0 WHERE thread_id = ?")
        .bind(enabled)
        .bind(thread_id)
        .execute(pool)
        .await?;

    Ok(())
}

pub async fn set_ai_support_paused(
    pool: &sqlx::SqlitePool,
    thread_id: i64,
    paused: bool,
) -> Result<(), anyhow::Error> {
    sqlx::query("UPDATE ai_support_threads SET paused = ? WHERE thread_id = ?")
        .bind(paused)
        .bind(thread_id)
        .execute(pool)
        .await?;

    Ok(())
}

pub async fn set_ai_support_resolved(
    pool: &sqlx::SqlitePool,
    thread_id: i64,
    resolved: bool,
) -> Result<(), anyhow::Error> {
    sqlx::query("UPDATE ai_support_threads SET resolved = ? WHERE thread_id = ?")
        .bind(resolved)
        .bind(thread_id)
        .execute(pool)
        .await?;

    Ok(())
}
