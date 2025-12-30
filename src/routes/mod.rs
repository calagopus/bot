use serde::Serialize;
use std::{sync::Arc, time::Instant};
use tokio::sync::RwLock;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;

mod github;

#[derive(ToSchema, Serialize)]
pub struct ApiError<'a> {
    pub errors: &'a [&'a str],
}

impl<'a> ApiError<'a> {
    pub fn new(errors: &'a [&'a str]) -> Self {
        Self { errors }
    }

    pub fn to_value(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap()
    }
}

#[derive(Debug)]
pub struct InnerState {
    pub start_time: Instant,
    pub version: String,

    pub env: Arc<crate::env::Env>,
    pub database: Arc<crate::database::Database>,
    pub bot: RwLock<Arc<serenity::http::Http>>,
}

pub type State = Arc<InnerState>;
pub type GetState = axum::extract::State<State>;

pub fn router(state: &State) -> OpenApiRouter<State> {
    OpenApiRouter::new()
        .nest("/github", github::router(state))
        .with_state(state.clone())
}
