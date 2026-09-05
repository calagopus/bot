use super::State;
use utoipa_axum::router::OpenApiRouter;

mod oauth;

pub fn router(state: &State) -> OpenApiRouter<State> {
    OpenApiRouter::new()
        .nest("/oauth", oauth::router(state))
        .with_state(state.clone())
}
