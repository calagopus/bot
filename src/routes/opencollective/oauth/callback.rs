use super::State;
use utoipa_axum::{router::OpenApiRouter, routes};

mod get {
    use crate::{
        response::ApiResponseResult,
        routes::{ApiError, GetState, oauth::Params},
        sponsors::Provider,
    };
    use axum::extract::Query;

    #[utoipa::path(get, path = "/", params(Params), responses(
        (status = OK, body = String, content_type = "text/html"),
        (status = BAD_REQUEST, body = ApiError),
        (status = SERVICE_UNAVAILABLE, body = ApiError),
    ))]
    pub async fn route(state: GetState, Query(params): Query<Params>) -> ApiResponseResult {
        crate::routes::oauth::callback(&state, Provider::OpenCollective, params).await
    }
}

pub fn router(state: &State) -> OpenApiRouter<State> {
    OpenApiRouter::new()
        .routes(routes!(get::route))
        .with_state(state.clone())
}
