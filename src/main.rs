use crate::routes::ApiError;
use axum::{
    ServiceExt,
    body::Body,
    extract::{ConnectInfo, Request},
    http::StatusCode,
    middleware::Next,
    response::Response,
    routing::get,
};
use colored::Colorize;
use sentry_tower::SentryHttpLayer;
use serenity::all::{GatewayIntents, Token};
use std::{net::SocketAddr, str::FromStr, sync::Arc, time::Instant};
use tikv_jemallocator::Jemalloc;
use tokio::sync::RwLock;
use tower::Layer;
use tower_http::{cors::CorsLayer, normalize_path::NormalizePathLayer};
use utoipa::openapi::security::{ApiKey, ApiKeyValue, SecurityScheme};
use utoipa_axum::router::OpenApiRouter;

mod commands;
mod components;
mod database;
mod env;
mod events;
mod modals;
mod models;
mod response;
mod routes;
mod utils;

#[global_allocator]
static ALLOC: Jemalloc = Jemalloc;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const GIT_COMMIT: &str = env!("CARGO_GIT_COMMIT");

pub type GetIp = axum::extract::Extension<std::net::IpAddr>;
pub type BotContext<'a> = poise::ApplicationContext<'a, routes::InnerState, anyhow::Error>;

async fn handle_request(
    connect_info: ConnectInfo<SocketAddr>,
    mut req: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let ip = crate::utils::extract_ip(req.headers()).unwrap_or_else(|| connect_info.ip());

    req.extensions_mut().insert(ip);

    tracing::info!(
        "http {} {}{}",
        req.method().to_string().to_lowercase(),
        req.uri().path().cyan(),
        if let Some(query) = req.uri().query() {
            format!("?{query}")
        } else {
            "".to_string()
        }
        .bright_cyan()
    );

    Ok(next.run(req).await)
}

#[tokio::main]
async fn main() {
    let (_env_guard, env) = env::Env::parse();

    let _guard = sentry::init((
        env.sentry_url.clone(),
        sentry::ClientOptions {
            server_name: env.server_name.clone().map(|s| s.into()),
            release: Some(format!("{VERSION}:{GIT_COMMIT}").into()),
            traces_sample_rate: 1.0,
            ..Default::default()
        },
    ));

    let env = Arc::new(env);
    let state = Arc::new(routes::InnerState {
        start_time: Instant::now(),
        version: format!("{VERSION}:{GIT_COMMIT}"),

        env: env.clone(),
        database: Arc::new(database::Database::new(env.clone()).await),
        bot: RwLock::new(Arc::new(serenity::http::Http::new(
            Token::from_str(&env.bot_token).unwrap(),
        ))),
    });

    let framework = poise::Framework::<routes::InnerState, anyhow::Error>::builder()
        .options(poise::FrameworkOptions {
            commands: commands::commands(commands::CommandList::new()).into_vec(),
            on_error: |err| {
                Box::pin(async move {
                    tracing::error!("bot encountered error: {:?}", err);
                })
            },
            pre_command: |ctx| {
                Box::pin(async move {
                    tracing::info!(
                        user = %ctx.author().name,
                        command = %ctx.command().qualified_name,
                        "bot executing command"
                    );
                })
            },
            post_command: |ctx| {
                Box::pin(async move {
                    tracing::info!(
                        command = %ctx.command().qualified_name,
                        "bot executed command"
                    );
                })
            },
            allowed_mentions: Some(serenity::all::CreateAllowedMentions::new().replied_user(true)),
            ..Default::default()
        })
        .build();
    let mut client = serenity::Client::builder(
        Token::from_str(&env.bot_token).unwrap(),
        GatewayIntents::non_privileged()
            | GatewayIntents::GUILD_MEMBERS
            | GatewayIntents::MESSAGE_CONTENT,
    )
    .data(state.clone())
    .raw_event_handler(Arc::new(events::EventHandler {
        components: components::components(components::ComponentList::new()),
    }))
    .framework(Box::new(framework))
    .await
    .unwrap();

    *state.bot.write().await = client.http.clone();

    tokio::spawn(async move {
        client.start().await.unwrap();
    });

    let app = OpenApiRouter::new()
        .nest("/api", routes::router(&state))
        .fallback(|| async move {
            Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header("Content-Type", "application/json")
                .body(Body::from(
                    ApiError::new(&["route not found"]).to_value().to_string(),
                ))
                .unwrap()
        })
        .layer(CorsLayer::very_permissive())
        .layer(axum::middleware::from_fn(handle_request))
        .route_layer(SentryHttpLayer::new().enable_transaction())
        .with_state(state.clone());

    let listener = tokio::net::TcpListener::bind(format!("{}:{}", &state.env.bind, state.env.port))
        .await
        .unwrap();

    tracing::info!(
        "{} listening on {} {}",
        "http server".bright_red(),
        state.env.bind.cyan(),
        format!(
            "(app@{}, {}ms)",
            VERSION,
            state.start_time.elapsed().as_millis()
        )
        .bright_black()
    );

    let (router, mut openapi) = app.split_for_parts();
    openapi.info.version = state.version.clone();
    openapi.info.description = None;
    openapi.info.title = "Blueprint API".to_string();
    openapi.info.contact = None;
    openapi.info.license = None;
    openapi.servers = Some(vec![utoipa::openapi::Server::new(
        state.env.app_url.clone(),
    )]);
    openapi.components.as_mut().unwrap().add_security_scheme(
        "api_key",
        SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::new("Authorization"))),
    );

    let router = router.route("/openapi.json", get(|| async move { axum::Json(openapi) }));

    axum::serve(
        listener,
        ServiceExt::<Request>::into_make_service_with_connect_info::<SocketAddr>(
            NormalizePathLayer::trim_trailing_slash().layer(router),
        ),
    )
    .await
    .unwrap();
}
