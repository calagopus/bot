use sqlx::{FromRow, Row, sqlite::SqliteRow};

#[derive(Debug)]
pub struct GithubUser {
    pub id: i64,
    pub login: Option<String>,
    pub discord_user_id: Option<i64>,
}

impl FromRow<'_, SqliteRow> for GithubUser {
    fn from_row(row: &SqliteRow) -> sqlx::Result<Self> {
        Ok(Self {
            id: row.try_get("id")?,
            login: row.try_get("login")?,
            discord_user_id: row.try_get("discord_user_id")?,
        })
    }
}

pub async fn find_github_user_by_discord(
    pool: &sqlx::SqlitePool,
    discord_user_id: i64,
) -> Result<Option<GithubUser>, anyhow::Error> {
    let user =
        sqlx::query_as::<_, GithubUser>("SELECT * FROM github_users WHERE discord_user_id = ?")
            .bind(discord_user_id)
            .fetch_optional(pool)
            .await?;

    Ok(user)
}

pub async fn linked_github_users(
    pool: &sqlx::SqlitePool,
) -> Result<Vec<GithubUser>, anyhow::Error> {
    let users = sqlx::query_as::<_, GithubUser>(
        "SELECT * FROM github_users WHERE discord_user_id IS NOT NULL",
    )
    .fetch_all(pool)
    .await?;

    Ok(users)
}

pub async fn upsert_github_user(
    pool: &sqlx::SqlitePool,
    id: i64,
    login: Option<&str>,
) -> Result<(), anyhow::Error> {
    sqlx::query(
        "INSERT INTO github_users (id, login) VALUES (?, ?)
         ON CONFLICT (id) DO UPDATE SET login = COALESCE(excluded.login, github_users.login)",
    )
    .bind(id)
    .bind(login)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn link_github_user(
    pool: &sqlx::SqlitePool,
    id: i64,
    login: &str,
    discord_user_id: i64,
) -> Result<(), anyhow::Error> {
    let mut transaction = pool.begin().await?;

    sqlx::query(
        "UPDATE github_users SET discord_user_id = NULL WHERE discord_user_id = ? AND id != ?",
    )
    .bind(discord_user_id)
    .bind(id)
    .execute(&mut *transaction)
    .await?;

    sqlx::query(
        "INSERT INTO github_users (id, login, discord_user_id) VALUES (?, ?, ?)
         ON CONFLICT (id) DO UPDATE SET login = excluded.login, discord_user_id = excluded.discord_user_id",
    )
    .bind(id)
    .bind(login)
    .bind(discord_user_id)
    .execute(&mut *transaction)
    .await?;

    transaction.commit().await?;

    Ok(())
}

pub async fn unlink_discord_user(
    pool: &sqlx::SqlitePool,
    discord_user_id: i64,
) -> Result<Option<GithubUser>, anyhow::Error> {
    let user = sqlx::query_as::<_, GithubUser>(
        "UPDATE github_users SET discord_user_id = NULL WHERE discord_user_id = ? RETURNING *",
    )
    .bind(discord_user_id)
    .fetch_optional(pool)
    .await?;

    Ok(user)
}
