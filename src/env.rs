use dotenvy::dotenv;
use tracing_subscriber::fmt::writer::MakeWriterExt;

#[derive(Debug, Clone)]
pub struct Env {
    pub sentry_url: Option<String>,

    pub database_url: String,
    pub database_migrate: bool,

    pub github_channel_id: u64,
    pub github_sponsors_channel_id: Option<u64>,
    pub github_verify_token: String,

    pub bot_token: String,

    pub bind: String,
    pub port: u16,

    pub app_debug: bool,
    pub app_log_directory: String,
    pub app_url: String,
    pub server_name: Option<String>,
}

impl Env {
    pub fn parse() -> (tracing_appender::non_blocking::WorkerGuard, Env) {
        dotenv().ok();

        let env = Self {
            sentry_url: std::env::var("SENTRY_URL")
                .ok()
                .map(|s| s.trim_matches('"').to_string()),

            database_url: std::env::var("DATABASE_URL")
                .expect("DATABASE_URL is required")
                .trim_matches('"')
                .to_string(),
            database_migrate: std::env::var("DATABASE_MIGRATE")
                .unwrap_or("false".to_string())
                .trim_matches('"')
                .parse()
                .unwrap(),

            github_channel_id: std::env::var("GITHUB_CHANNEL_ID")
                .expect("GITHUB_CHANNEL_ID is required")
                .trim_matches('"')
                .parse()
                .unwrap(),
            github_sponsors_channel_id: std::env::var("GITHUB_SPONSORS_CHANNEL_ID")
                .map(|c| c.trim_matches('"').to_string())
                .unwrap_or("".to_string())
                .parse()
                .ok(),
            github_verify_token: std::env::var("GITHUB_VERIFY_TOKEN")
                .expect("GITHUB_VERIFY_TOKEN is required")
                .trim_matches('"')
                .to_string(),

            bot_token: std::env::var("BOT_TOKEN")
                .expect("BOT_TOKEN is required")
                .trim_matches('"')
                .to_string(),

            bind: std::env::var("BIND")
                .unwrap_or("0.0.0.0".to_string())
                .trim_matches('"')
                .to_string(),
            port: std::env::var("PORT")
                .unwrap_or("6969".to_string())
                .parse()
                .unwrap(),

            app_debug: std::env::var("APP_DEBUG")
                .unwrap_or("false".to_string())
                .trim_matches('"')
                .parse()
                .unwrap(),
            app_log_directory: std::env::var("APP_LOG_DIRECTORY")
                .unwrap_or("logs".to_string())
                .trim_matches('"')
                .to_string(),
            app_url: std::env::var("APP_URL")
                .expect("APP_URL is required")
                .trim_matches('"')
                .to_string(),
            server_name: std::env::var("SERVER_NAME")
                .ok()
                .map(|s| s.trim_matches('"').to_string()),
        };

        if !std::path::Path::new(&env.app_log_directory).exists() {
            std::fs::create_dir_all(&env.app_log_directory)
                .expect("failed to create log directory");
        }

        let latest_log_path = std::path::Path::new(&env.app_log_directory).join("bot.log");
        let latest_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&latest_log_path)
            .expect("failed to open latest log file");

        let rolling_appender = tracing_appender::rolling::Builder::new()
            .filename_prefix("bot")
            .filename_suffix("log")
            .max_log_files(30)
            .rotation(tracing_appender::rolling::Rotation::DAILY)
            .build(&env.app_log_directory)
            .expect("failed to create rolling log file appender");

        let (file_appender, _guard) = tracing_appender::non_blocking::NonBlockingBuilder::default()
            .buffered_lines_limit(50)
            .lossy(false)
            .finish(latest_file.and(rolling_appender));

        tracing::subscriber::set_global_default(
            tracing_subscriber::fmt()
                .with_timer(tracing_subscriber::fmt::time::ChronoLocal::rfc_3339())
                .with_writer(std::io::stdout.and(file_appender))
                .with_target(false)
                .with_level(true)
                .with_file(true)
                .with_line_number(true)
                .with_max_level(if env.app_debug {
                    tracing::Level::DEBUG
                } else {
                    tracing::Level::INFO
                })
                .finish(),
        )
        .unwrap();

        (_guard, env)
    }
}
