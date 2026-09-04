use super::State;
use utoipa_axum::router::OpenApiRouter;

mod callback;

pub fn router(state: &State) -> OpenApiRouter<State> {
    OpenApiRouter::new()
        .nest("/callback", callback::router(state))
        .with_state(state.clone())
}
