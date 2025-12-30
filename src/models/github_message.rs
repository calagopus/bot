use chrono::DateTime;
use octocrab::models::webhook_events::payload::PushWebhookEventCommit;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Row, sqlite::SqliteRow};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
pub struct WorkflowStatus {
    pub name: String,
    pub status: octocrab::models::workflows::Status,
    pub started: DateTime<chrono::Utc>,
}

#[derive(Debug)]
pub struct GithubMessage {
    pub id: i64,
    pub message_id: i64,

    pub commits: Vec<PushWebhookEventCommit>,

    pub workflow_status: HashMap<i64, WorkflowStatus>,
}

impl FromRow<'_, SqliteRow> for GithubMessage {
    fn from_row(row: &SqliteRow) -> sqlx::Result<Self> {
        Ok(Self {
            id: row.try_get("id")?,
            message_id: row.try_get("message_id")?,
            commits: serde_json::from_str(&row.try_get::<String, _>("commits")?).map_err(|e| {
                sqlx::Error::ColumnDecode {
                    index: "commits".into(),
                    source: Box::new(e),
                }
            })?,
            workflow_status: serde_json::from_str(&row.try_get::<String, _>("workflow_status")?)
                .map_err(|e| sqlx::Error::ColumnDecode {
                    index: "workflow_status".into(),
                    source: Box::new(e),
                })?,
        })
    }
}
