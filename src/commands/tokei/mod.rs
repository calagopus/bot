use poise::CreateReply;
use serde::Deserialize;
use serenity::all::{
    AutocompleteChoice, ButtonStyle, CreateActionRow, CreateAttachment, CreateAutocompleteResponse,
    CreateButton, CreateComponent, CreateContainer, CreateContainerComponent, CreateMediaGallery,
    CreateMediaGalleryItem, CreateSection, CreateSectionAccessory, CreateSectionComponent,
    CreateSeparator, CreateTextDisplay, CreateThumbnail, CreateUnfurledMediaItem, MessageFlags,
    ReactionType,
};
use std::sync::{Arc, LazyLock};

mod colors;
mod render;

const ORGANIZATION: &str = "calagopus";
const CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(3600);
const MAX_LANGUAGES: usize = 10;
const REFRESH_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(5 * 60);
const REFRESH_COOLDOWN_UNAUTHENTICATED: std::time::Duration =
    std::time::Duration::from_secs(15 * 60);
const EMOJI_REFRESH_ID: u64 = 1150889388834820249;

const LOGO_URL: &str = "https://calagopus.com/logo.png";

const EMOJI_STAR: &str = "<:star:1229766059381358623>";
const EMOJI_HAMMER: &str = "<:hammer:1150889684227076227>";
const EMOJI_PACKAGE: &str = "<:package:1150890021516234832>";
const EMOJI_STORAGE: &str = "<:storage:1150889889294991381>";
const EMOJI_ISSUES: &str = "<:issues:1169252441347334184>";
const EMOJI_CLOCK: &str = "<:clock:1150889651914158111>";
const EMOJI_REFRESH: &str = "<:refresh:1150889388834820249>";

static CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .user_agent(format!("Calagopus Bot ({})", crate::VERSION))
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .expect("failed to build tokei http client")
});

static REPOSITORIES_CACHE: LazyLock<moka::future::Cache<(), Arc<Vec<Repository>>>> =
    LazyLock::new(|| {
        moka::future::Cache::builder()
            .time_to_live(CACHE_TTL)
            .max_capacity(1)
            .build()
    });

static STATS_CACHE: LazyLock<moka::future::Cache<String, Arc<RepositoryStats>>> =
    LazyLock::new(|| {
        moka::future::Cache::builder()
            .time_to_live(CACHE_TTL)
            .max_capacity(100)
            .build()
    });

#[derive(Clone, Deserialize)]
struct RepositoryLicense {
    spdx_id: Option<String>,
}

#[derive(Clone, Deserialize)]
struct Repository {
    name: String,
    description: Option<String>,
    html_url: String,
    language: Option<String>,
    license: Option<RepositoryLicense>,
    default_branch: String,
    stargazers_count: u64,
    forks_count: u64,
    open_issues_count: u64,
    archived: bool,
    pushed_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
}

struct LanguageStats {
    name: String,
    files: usize,
    code: usize,
    comments: usize,
    blanks: usize,
}

pub struct RepositoryStats {
    repository: Repository,
    latest_release: Option<Release>,
    commits: u64,
    languages: Vec<LanguageStats>,
    total: LanguageStats,
    fetched_at: i64,
    expires: i64,
    images: Option<render::LanguageImages>,
}

fn github_get(env: &crate::env::Env, url: &str) -> reqwest::RequestBuilder {
    let request = CLIENT
        .get(url)
        .header("Accept", "application/vnd.github+json");

    match &env.github_token {
        Some(github_token) => request.bearer_auth(github_token),
        None => request,
    }
}

async fn collect_repositories(env: &crate::env::Env) -> Result<Vec<Repository>, anyhow::Error> {
    let mut repositories = Vec::new();

    for page in 1.. {
        let batch: Vec<Repository> = github_get(
            env,
            &format!(
                "https://api.github.com/orgs/{ORGANIZATION}/repos?type=public&per_page=100&page={page}"
            ),
        )
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

        let done = batch.len() < 100;
        repositories.extend(batch);

        if done {
            break;
        }
    }

    repositories.sort_unstable_by_key(|r| std::cmp::Reverse(r.pushed_at));

    Ok(repositories)
}

async fn cached_repositories(env: &crate::env::Env) -> Result<Arc<Vec<Repository>>, anyhow::Error> {
    REPOSITORIES_CACHE
        .try_get_with((), async { collect_repositories(env).await.map(Arc::new) })
        .await
        .map_err(|err| anyhow::anyhow!("{err}"))
}

async fn collect_commit_count(
    env: &crate::env::Env,
    repository: &Repository,
) -> Result<u64, anyhow::Error> {
    let response = github_get(
        env,
        &format!(
            "https://api.github.com/repos/{ORGANIZATION}/{}/commits?sha={}&per_page=1",
            repository.name, repository.default_branch
        ),
    )
    .send()
    .await?
    .error_for_status()?;

    let last_page = response
        .headers()
        .get(reqwest::header::LINK)
        .and_then(|link| link.to_str().ok())
        .and_then(|link| {
            link.split(',')
                .find(|part| part.contains("rel=\"last\""))
                .and_then(|part| Some(&part[part.find('<')? + 1..part.find('>')?]))
                .and_then(|url| reqwest::Url::parse(url).ok())
                .and_then(|url| {
                    url.query_pairs()
                        .find(|(key, _)| key == "page")
                        .and_then(|(_, page)| page.parse().ok())
                })
        });

    match last_page {
        Some(last_page) => Ok(last_page),
        None => Ok(response.json::<Vec<serde_json::Value>>().await?.len() as u64),
    }
}

async fn collect_latest_release(
    env: &crate::env::Env,
    repository: &Repository,
) -> Result<Option<Release>, anyhow::Error> {
    let response = github_get(
        env,
        &format!(
            "https://api.github.com/repos/{ORGANIZATION}/{}/releases/latest",
            repository.name
        ),
    )
    .send()
    .await?;

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }

    Ok(Some(response.error_for_status()?.json().await?))
}

async fn collect_languages(
    env: &crate::env::Env,
    repository: &Repository,
) -> Result<Vec<LanguageStats>, anyhow::Error> {
    let tarball = github_get(
        env,
        &format!(
            "https://api.github.com/repos/{ORGANIZATION}/{}/tarball/{}",
            repository.name, repository.default_branch
        ),
    )
    .send()
    .await?
    .error_for_status()?
    .bytes()
    .await?;

    tokio::task::spawn_blocking(move || {
        let directory = tempfile::tempdir()?;
        tar::Archive::new(flate2::read::GzDecoder::new(tarball.as_ref()))
            .unpack(directory.path())?;

        let mut languages = tokei::Languages::new();
        languages.get_statistics(&[directory.path()], &[], &tokei::Config::default());

        let mut languages = languages
            .iter()
            .map(|(language_type, language)| {
                let files = language.reports.len();
                let language = language.summarise();

                LanguageStats {
                    name: language_type.name().to_string(),
                    files,
                    code: language.code,
                    comments: language.comments,
                    blanks: language.blanks,
                }
            })
            .filter(|language| language.files > 0)
            .collect::<Vec<_>>();
        languages.sort_unstable_by_key(|l| std::cmp::Reverse(l.code));

        Ok(languages)
    })
    .await?
}

async fn collect_stats(
    env: &crate::env::Env,
    name: &str,
) -> Result<RepositoryStats, anyhow::Error> {
    let repository: Repository = github_get(
        env,
        &format!("https://api.github.com/repos/{ORGANIZATION}/{name}"),
    )
    .send()
    .await?
    .error_for_status()?
    .json()
    .await?;

    let (latest_release, commits, languages) = tokio::try_join!(
        collect_latest_release(env, &repository),
        collect_commit_count(env, &repository),
        collect_languages(env, &repository)
    )?;

    let total = languages.iter().fold(
        LanguageStats {
            name: "Total".to_string(),
            files: 0,
            code: 0,
            comments: 0,
            blanks: 0,
        },
        |mut total, language| {
            total.files += language.files;
            total.code += language.code;
            total.comments += language.comments;
            total.blanks += language.blanks;
            total
        },
    );

    let mut stats = RepositoryStats {
        repository,
        latest_release,
        commits,
        languages,
        total,
        fetched_at: chrono::Utc::now().timestamp(),
        expires: (chrono::Utc::now() + CACHE_TTL).timestamp(),
        images: None,
    };
    if !stats.languages.is_empty() {
        stats.images = Some(render::render_images(&stats).await?);
    }

    Ok(stats)
}

fn format_number(number: impl ToString) -> String {
    let digits = number.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);

    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            formatted.push(',');
        }
        formatted.push(digit);
    }

    formatted
}

fn format_share(part: usize, total: usize) -> String {
    format!("{:.1}%", part as f64 * 100.0 / total.max(1) as f64)
}

fn repository_header(stats: &RepositoryStats) -> String {
    let repository = &stats.repository;

    let mut header = format!(
        "## [{ORGANIZATION}/{}]({})\n{}",
        repository.name,
        repository.html_url,
        repository
            .description
            .as_deref()
            .map(str::trim)
            .filter(|description| !description.is_empty())
            .unwrap_or("*No description provided.*"),
    );

    match repository
        .license
        .as_ref()
        .and_then(|license| license.spdx_id.as_deref())
    {
        Some("NOASSERTION") => header.push_str("\n⚖️ Licensed under a custom license"),
        Some(spdx_id) => header.push_str(&format!("\n⚖️ Licensed under **{spdx_id}**")),
        None => header.push_str("\n⚖️ No license"),
    }

    let mut tags = Vec::new();
    if let Some(release) = &stats.latest_release {
        tags.push(format!(
            "{EMOJI_PACKAGE} [`{}`]({})",
            release.tag_name, release.html_url
        ));
    }
    if repository.default_branch != "main" {
        tags.push(format!("🌿 `{}`", repository.default_branch));
    }
    if repository.archived {
        tags.push("🗄️ archived".to_string());
    }
    if !tags.is_empty() {
        header.push_str(&format!("\n-# {}", tags.join(" · ")));
    }

    header
}

fn repository_details(stats: &RepositoryStats) -> String {
    let repository = &stats.repository;

    let mut details = format!(
        "{EMOJI_STAR} **{}** stars · {EMOJI_HAMMER} **{}** forks · {EMOJI_ISSUES} **{}** open issues & PRs · {EMOJI_STORAGE} **{}** commits",
        format_number(repository.stargazers_count),
        format_number(repository.forks_count),
        format_number(repository.open_issues_count),
        format_number(stats.commits),
    );

    if let Some(pushed_at) = repository.pushed_at {
        details.push_str(&format!(
            "\n{EMOJI_CLOCK} Last push <t:{}:R>",
            pushed_at.timestamp()
        ));
    }

    details
}

fn image_gallery<'a>(filename: &str, description: String) -> CreateContainerComponent<'a> {
    CreateContainerComponent::MediaGallery(CreateMediaGallery::new(vec![
        CreateMediaGalleryItem::new(CreateUnfurledMediaItem::new(format!(
            "attachment://{filename}"
        )))
        .description(description),
    ]))
}

pub enum MessagePart {
    Components(Vec<CreateComponent<'static>>),
    Attachment(CreateAttachment<'static>),
}

/// The full message for `stats`, shared by the slash command and the refresh button.
pub fn message_components(stats: &RepositoryStats) -> Vec<MessagePart> {
    let repository = &stats.repository;
    let name = &repository.name;
    let mut parts = Vec::new();

    let accent_color = if repository.archived {
        0x6e7681
    } else {
        colors::language_color(
            repository
                .language
                .as_deref()
                .or_else(|| stats.languages.first().map(|l| l.name.as_str()))
                .unwrap_or_default(),
        )
    };

    let mut buttons = vec![CreateButton::new_link(repository.html_url.clone()).label("Repository")];
    if stats.latest_release.is_some() {
        buttons.push(
            CreateButton::new_link(format!("{}/releases", repository.html_url)).label("Releases"),
        );
    }
    buttons.push(CreateButton::new_link(format!("{}/issues", repository.html_url)).label("Issues"));
    buttons.push(
        CreateButton::new(format!(
            "{}{name}:{}",
            crate::components::TOKEI_REFRESH_ID,
            stats.fetched_at
        ))
        .label("Refresh")
        .style(ButtonStyle::Secondary)
        .emoji(ReactionType::Custom {
            animated: false,
            id: EMOJI_REFRESH_ID.into(),
            name: None,
        }),
    );

    let mut container_components = vec![
        CreateContainerComponent::Section(CreateSection::new(
            vec![CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                repository_header(stats),
            ))],
            CreateSectionAccessory::Thumbnail(CreateThumbnail::new(CreateUnfurledMediaItem::new(
                LOGO_URL,
            ))),
        )),
        CreateContainerComponent::TextDisplay(CreateTextDisplay::new(repository_details(stats))),
        CreateContainerComponent::Separator(CreateSeparator::new()),
    ];

    match &stats.images {
        Some(images) => {
            let overview = format!("tokei-{name}-overview.png");
            let table = format!("tokei-{name}-languages.png");

            container_components.push(image_gallery(
                &overview,
                format!("Lines of code in {ORGANIZATION}/{name} by language"),
            ));
            container_components.push(CreateContainerComponent::Separator(CreateSeparator::new()));
            container_components.push(image_gallery(
                &table,
                format!("Per-language code statistics for {ORGANIZATION}/{name}"),
            ));

            parts.push(MessagePart::Attachment(CreateAttachment::bytes(
                images.overview.clone(),
                overview,
            )));
            parts.push(MessagePart::Attachment(CreateAttachment::bytes(
                images.table.clone(),
                table,
            )));
        }
        None => container_components.push(CreateContainerComponent::TextDisplay(
            CreateTextDisplay::new("### No code found\n-# tokei did not recognize any files."),
        )),
    }

    container_components.push(CreateContainerComponent::Separator(CreateSeparator::new()));
    container_components.push(CreateContainerComponent::ActionRow(
        CreateActionRow::Buttons(buttons.into()),
    ));
    container_components.push(CreateContainerComponent::TextDisplay(
        CreateTextDisplay::new(format!(
            "-# {EMOJI_REFRESH} Counted with tokei <t:{}:R> · cached for 1 hour, expires <t:{}:R>",
            stats.fetched_at, stats.expires
        )),
    ));

    parts.push(MessagePart::Components(vec![CreateComponent::Container(
        CreateContainer::new(container_components).accent_color(accent_color),
    )]));

    parts
}

pub enum Refresh {
    Updated(Arc<RepositoryStats>),
    Cooldown { fetched_at: i64, available_at: i64 },
}

fn refresh_cooldown(env: &crate::env::Env) -> std::time::Duration {
    if env.github_token.is_some() {
        REFRESH_COOLDOWN
    } else {
        REFRESH_COOLDOWN_UNAUTHENTICATED
    }
}

pub async fn refresh(
    env: &crate::env::Env,
    repository: &str,
    shown_fetched_at: i64,
) -> Result<Option<Refresh>, anyhow::Error> {
    let Some(repository) = cached_repositories(env)
        .await?
        .iter()
        .find(|r| r.name == repository)
        .map(|r| r.name.clone())
    else {
        return Ok(None);
    };

    let cooldown = refresh_cooldown(env).as_secs() as i64;

    let result = STATS_CACHE
        .entry(repository.clone())
        .and_try_compute_with(|entry| async {
            if let Some(entry) = entry
                && chrono::Utc::now().timestamp() - entry.value().fetched_at < cooldown
            {
                return Ok(moka::ops::compute::Op::Nop);
            }

            collect_stats(env, &repository)
                .await
                .map(|stats| moka::ops::compute::Op::Put(Arc::new(stats)))
        })
        .await?;

    let stats = match result {
        moka::ops::compute::CompResult::Inserted(entry)
        | moka::ops::compute::CompResult::ReplacedWith(entry)
        | moka::ops::compute::CompResult::Unchanged(entry) => entry.into_value(),
        moka::ops::compute::CompResult::StillNone(_)
        | moka::ops::compute::CompResult::Removed(_) => {
            return Err(anyhow::anyhow!(
                "tokei stats for {repository} were not cached"
            ));
        }
    };

    if stats.fetched_at == shown_fetched_at {
        return Ok(Some(Refresh::Cooldown {
            fetched_at: stats.fetched_at,
            available_at: stats.fetched_at + cooldown,
        }));
    }

    Ok(Some(Refresh::Updated(stats)))
}

async fn autocomplete_repository<'a>(
    ctx: crate::BotContext<'_>,
    partial: &'a str,
) -> CreateAutocompleteResponse<'a> {
    let mut response = CreateAutocompleteResponse::new();

    let repositories = match cached_repositories(&ctx.data().env).await {
        Ok(repositories) => repositories,
        Err(err) => {
            tracing::error!("failed to fetch github repositories: {:?}", err);
            return response;
        }
    };

    let partial = partial.to_lowercase();
    for repository in repositories
        .iter()
        .filter(|repository| repository.name.to_lowercase().contains(&partial))
        .take(25)
    {
        response = response.add_choice(AutocompleteChoice::new(
            repository.name.clone(),
            repository.name.clone(),
        ));
    }

    response
}

/// Show code statistics for a Calagopus repository.
#[poise::command(slash_command, rename = "tokei")]
pub async fn tokei_command(
    ctx: crate::BotContext<'_>,
    #[description = "The repository to count"]
    #[autocomplete = "autocomplete_repository"]
    repository: String,
) -> Result<(), anyhow::Error> {
    let env = &ctx.data().env;

    let Some(repository) = cached_repositories(env)
        .await?
        .iter()
        .find(|r| r.name.eq_ignore_ascii_case(&repository))
        .map(|r| r.name.clone())
    else {
        ctx.send(
            CreateReply::default()
                .content(format!("That is not a public {ORGANIZATION} repository."))
                .ephemeral(true),
        )
        .await?;
        return Ok(());
    };

    let stats = match STATS_CACHE.get(&repository).await {
        Some(stats) => stats,
        None => {
            ctx.defer().await?;

            STATS_CACHE
                .try_get_with(repository.clone(), async {
                    collect_stats(env, &repository).await.map(Arc::new)
                })
                .await
                .map_err(|err| anyhow::anyhow!("{err}"))?
        }
    };

    ctx.send(
        message_components(&stats)
            .into_iter()
            .fold(CreateReply::default(), |reply, part| match part {
                MessagePart::Components(components) => reply.components(components),
                MessagePart::Attachment(attachment) => reply.attachment(attachment),
            })
            .flags(MessageFlags::IS_COMPONENTS_V2),
    )
    .await?;

    Ok(())
}
