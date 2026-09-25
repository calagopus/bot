use super::State;
use utoipa_axum::{router::OpenApiRouter, routes};

mod post {
    use crate::{
        response::ApiResponseResult,
        routes::{ApiError, GetState, oauth::ConfirmParams},
        sponsors::Provider,
    };
    use axum::Form;

    #[utoipa::path(post, path = "/", responses(
        (status = OK, body = String, content_type = "text/html"),
        (status = BAD_REQUEST, body = ApiError),
        (status = SERVICE_UNAVAILABLE, body = ApiError),
    ), request_body(content = ConfirmParams, content_type = "application/x-www-form-urlencoded"))]
    pub async fn route(state: GetState, Form(params): Form<ConfirmParams>) -> ApiResponseResult {
        crate::routes::oauth::confirm(&state, Provider::GitHub, params).await
    }
}

pub fn router(state: &State) -> OpenApiRouter<State> {
    OpenApiRouter::new()
        .routes(routes!(post::route))
        .with_state(state.clone())
}
