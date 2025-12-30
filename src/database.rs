use colored::Colorize;
use std::{str::FromStr, sync::Arc};

#[derive(Debug)]
pub struct Database {
    sqlite: sqlx::SqlitePool,
}

impl Database {
    pub async fn new(env: Arc<crate::env::Env>) -> Self {
        if let Some(parent) =
            std::path::Path::new(env.database_url.trim_start_matches("sqlite:")).parent()
        {
            std::fs::create_dir_all(parent).unwrap();
        }

        let instance = Self {
            sqlite: sqlx::sqlite::SqlitePool::connect_with(
                sqlx::sqlite::SqliteConnectOptions::from_str(&env.database_url)
                    .unwrap()
                    .create_if_missing(true)
                    .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal),
            )
            .await
            .expect("Failed to connect to the database"),
        };

        if env.database_migrate {
            let writer = instance.sqlite.clone();
            tokio::spawn(async move {
                let start = std::time::Instant::now();

                sqlx::migrate!("./database/migrations")
                    .run(&writer)
                    .await
                    .unwrap();

                tracing::info!(
                    "{} migrated {}",
                    "database".bright_cyan(),
                    format!("({}ms)", start.elapsed().as_millis()).bright_black()
                );
            });
        }

        instance
    }

    #[inline]
    pub fn write(&self) -> &sqlx::SqlitePool {
        &self.sqlite
    }

    #[inline]
    pub fn read(&self) -> &sqlx::SqlitePool {
        &self.sqlite
    }
}
