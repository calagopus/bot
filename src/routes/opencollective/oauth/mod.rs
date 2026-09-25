use super::State;
use utoipa_axum::router::OpenApiRouter;

mod callback;
mod confirm;

pub fn router(state: &State) -> OpenApiRouter<State> {
    OpenApiRouter::new()
        .nest("/callback", callback::router(state))
        .nest("/confirm", confirm::router(state))
        .with_state(state.clone())
}
