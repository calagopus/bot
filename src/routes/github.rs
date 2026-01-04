use super::State;
use utoipa_axum::{router::OpenApiRouter, routes};

mod post {
    use crate::{
        models::MESSAGE_LOCK,
        response::{ApiResponse, ApiResponseResult},
        routes::{ApiError, GetState},
    };
    use axum::http::StatusCode;
    use hmac::Mac;
    use octocrab::models::webhook_events::{
        WebhookEventPayload,
        payload::{
            IssuesWebhookEventAction, PullRequestWebhookEventAction, StarWebhookEventAction,
        },
    };
    use serde::{Deserialize, Serialize};
    use serenity::all::{
        CreateButton, CreateComponent, CreateContainer, CreateContainerComponent, CreateMessage,
        CreateSection, CreateSectionAccessory, CreateSectionComponent, CreateSeparator,
        CreateTextDisplay, CreateThumbnail, CreateUnfurledMediaItem, MessageFlags,
    };
    use utoipa::ToSchema;

    #[derive(ToSchema, Serialize)]
    struct Response {}

    #[utoipa::path(post, path = "/", responses(
        (status = OK, body = inline(Response)),
        (status = BAD_REQUEST, body = ApiError),
        (status = UNAUTHORIZED, body = ApiError),
    ), request_body = String)]
    pub async fn route(
        state: GetState,
        headers: axum::http::HeaderMap,
        data: axum::body::Bytes,
    ) -> ApiResponseResult {
        let Some(signature_header) = headers.get("X-Hub-Signature-256") else {
            return ApiResponse::error("missing X-Hub-Signature-256 header")
                .with_status(StatusCode::UNAUTHORIZED)
                .ok();
        };

        let signature_str = signature_header.to_str().unwrap_or_default();
        if !signature_str.starts_with("sha256=") {
            return ApiResponse::error("invalid signature format")
                .with_status(StatusCode::BAD_REQUEST)
                .ok();
        }

        let signature_hex = &signature_str["sha256=".len()..];
        let signature_bytes = match hex::decode(signature_hex) {
            Ok(bytes) => bytes,
            Err(_) => {
                return ApiResponse::error("invalid signature hex")
                    .with_status(StatusCode::BAD_REQUEST)
                    .ok();
            }
        };

        let mut mac: hmac::Hmac<sha2::Sha256> =
            hmac::Hmac::new_from_slice(state.env.github_verify_token.as_bytes()).map_err(|e| {
                ApiResponse::error(&e.to_string()).with_status(StatusCode::INTERNAL_SERVER_ERROR)
            })?;
        mac.update(&data);

        if mac.verify_slice(&signature_bytes).is_err() {
            return ApiResponse::error("invalid webhook signature")
                .with_status(StatusCode::UNAUTHORIZED)
                .ok();
        }

        let Some(event) = headers.get("X-GitHub-Event") else {
            return ApiResponse::error("missing X-GitHub-Event header")
                .with_status(StatusCode::BAD_REQUEST)
                .ok();
        };

        let event = octocrab::models::webhook_events::WebhookEvent::try_from_header_and_body(
            event.to_str()?,
            &data,
        )?;

        let Some(organization) = event.organization else {
            return ApiResponse::error("missing organization information in webhook payload")
                .with_status(StatusCode::BAD_REQUEST)
                .ok();
        };
        let Some(repository) = event.repository else {
            return ApiResponse::error("missing repository information in webhook payload")
                .with_status(StatusCode::BAD_REQUEST)
                .ok();
        };
        let Some(sender) = event.sender else {
            return ApiResponse::error("missing sender information in webhook payload")
                .with_status(StatusCode::BAD_REQUEST)
                .ok();
        };

        let mut container_components = Vec::new();
        let mut edit_github_message = None;
        let mut create_github_message = None;

        match event.specific {
            WebhookEventPayload::Push(push) => {
                let mut commit_string = String::new();

                for commit in push.commits.iter() {
                    commit_string.push_str(&format!(
                        "[`{}`]({}): {}\n",
                        commit.id.chars().take(7).collect::<String>(),
                        commit.url,
                        commit.message.lines().next().unwrap_or_default()
                    ));
                }

                container_components.push(CreateContainerComponent::Section(CreateSection::new(
                    vec![
                        CreateSectionComponent::TextDisplay(CreateTextDisplay::new(format!(
                            "## <:package:1150890021516234832> {} Commit{} pushed",
                            push.commits.len(),
                            if push.commits.len() == 1 { "" } else { "s" }
                        ))),
                        CreateSectionComponent::TextDisplay(CreateTextDisplay::new(commit_string)),
                    ],
                    CreateSectionAccessory::Thumbnail(CreateThumbnail::new(
                        CreateUnfurledMediaItem::new(organization.avatar_url.to_string()),
                    )),
                )));

                if let Some(head_commit) = push.head_commit {
                    create_github_message = Some((head_commit, push.commits));
                }
            }
            WebhookEventPayload::Star(star) => match star.action {
                StarWebhookEventAction::Created => {
                    container_components.push(CreateContainerComponent::Section(
                        CreateSection::new(
                            vec![
                                CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                                    "## <:star:1229766059381358623> Repository starred",
                                )),
                                CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                                    format!(
                                        "[**{}**]({}) starred the repository!",
                                        sender.login, sender.html_url,
                                    ),
                                )),
                                CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                                    format!(
                                        "The new star count is `{}`.",
                                        repository.stargazers_count.unwrap_or(0)
                                    ),
                                )),
                            ],
                            CreateSectionAccessory::Thumbnail(CreateThumbnail::new(
                                CreateUnfurledMediaItem::new(organization.avatar_url.to_string()),
                            )),
                        ),
                    ));
                }
                StarWebhookEventAction::Deleted => {
                    container_components.push(CreateContainerComponent::Section(
                        CreateSection::new(
                            vec![
                                CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                                    "## <:star:1229766059381358623> Repository unstarred",
                                )),
                                CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                                    format!(
                                        "[**{}**]({}) unstarred the repository!",
                                        sender.login, sender.html_url,
                                    ),
                                )),
                                CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                                    format!(
                                        "The new star count is `{}`.",
                                        repository.stargazers_count.unwrap_or(0)
                                    ),
                                )),
                            ],
                            CreateSectionAccessory::Thumbnail(CreateThumbnail::new(
                                CreateUnfurledMediaItem::new(organization.avatar_url.to_string()),
                            )),
                        ),
                    ));
                }
                _ => {}
            },
            WebhookEventPayload::PullRequest(pull_request) => match pull_request.action {
                PullRequestWebhookEventAction::Opened => {
                    container_components.push(CreateContainerComponent::Section(
                        CreateSection::new(
                            vec![
                                CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                                    "## <:storage:1150889889294991381> Pull Request opened",
                                )),
                                CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                                    format!(
                                        "[**{}**]({}) opened a new pull request:",
                                        sender.login, sender.html_url,
                                    ),
                                )),
                                CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                                    format!(
                                        "[`#{} {}`]({})",
                                        pull_request.number,
                                        pull_request.pull_request.title.unwrap_or_default(),
                                        pull_request
                                            .pull_request
                                            .html_url
                                            .map_or_else(|| "".to_string(), |url| url.to_string()),
                                    ),
                                )),
                            ],
                            CreateSectionAccessory::Thumbnail(CreateThumbnail::new(
                                CreateUnfurledMediaItem::new(organization.avatar_url.to_string()),
                            )),
                        ),
                    ));
                }
                PullRequestWebhookEventAction::Reopened => {
                    container_components.push(CreateContainerComponent::Section(
                        CreateSection::new(
                            vec![
                                CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                                    "## <:storage:1150889889294991381> Pull Request reopened",
                                )),
                                CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                                    format!(
                                        "[**{}**]({}) reopened a pull request:",
                                        sender.login, sender.html_url,
                                    ),
                                )),
                                CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                                    format!(
                                        "[`#{} {}`]({})",
                                        pull_request.number,
                                        pull_request.pull_request.title.unwrap_or_default(),
                                        pull_request
                                            .pull_request
                                            .html_url
                                            .map_or_else(|| "".to_string(), |url| url.to_string()),
                                    ),
                                )),
                            ],
                            CreateSectionAccessory::Thumbnail(CreateThumbnail::new(
                                CreateUnfurledMediaItem::new(organization.avatar_url.to_string()),
                            )),
                        ),
                    ));
                }
                PullRequestWebhookEventAction::Closed => {
                    container_components.push(CreateContainerComponent::Section(
                        CreateSection::new(
                            vec![
                                CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                                    "## <:storage:1150889889294991381> Pull Request closed",
                                )),
                                CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                                    format!(
                                        "[**{}**]({}) closed a pull request:",
                                        sender.login, sender.html_url,
                                    ),
                                )),
                                CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                                    format!(
                                        "[`#{} {}`]({})",
                                        pull_request.number,
                                        pull_request.pull_request.title.unwrap_or_default(),
                                        pull_request
                                            .pull_request
                                            .html_url
                                            .map_or_else(|| "".to_string(), |url| url.to_string()),
                                    ),
                                )),
                            ],
                            CreateSectionAccessory::Thumbnail(CreateThumbnail::new(
                                CreateUnfurledMediaItem::new(organization.avatar_url.to_string()),
                            )),
                        ),
                    ));
                }
                _ => {}
            },
            WebhookEventPayload::Issues(issue) => {
                // same as pull request but for issues
                match issue.action {
                    IssuesWebhookEventAction::Opened => {
                        container_components.push(CreateContainerComponent::Section(
                            CreateSection::new(
                                vec![
                                    CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                                        "## <:hammer:1150889684227076227> Issue opened",
                                    )),
                                    CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                                        format!(
                                            "[**{}**]({}) opened a new issue:",
                                            sender.login, sender.html_url,
                                        ),
                                    )),
                                    CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                                        format!(
                                            "[`#{} {}`]({})",
                                            issue.issue.number,
                                            issue.issue.title,
                                            issue.issue.html_url
                                        ),
                                    )),
                                ],
                                CreateSectionAccessory::Thumbnail(CreateThumbnail::new(
                                    CreateUnfurledMediaItem::new(
                                        organization.avatar_url.to_string(),
                                    ),
                                )),
                            ),
                        ));
                    }
                    IssuesWebhookEventAction::Reopened => {
                        container_components.push(CreateContainerComponent::Section(
                            CreateSection::new(
                                vec![
                                    CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                                        "## <:hammer:1150889684227076227> Issue reopened",
                                    )),
                                    CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                                        format!(
                                            "[**{}**]({}) reopened an issue:",
                                            sender.login, sender.html_url,
                                        ),
                                    )),
                                    CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                                        format!(
                                            "[`#{} {}`]({})",
                                            issue.issue.number,
                                            issue.issue.title,
                                            issue.issue.html_url
                                        ),
                                    )),
                                ],
                                CreateSectionAccessory::Thumbnail(CreateThumbnail::new(
                                    CreateUnfurledMediaItem::new(
                                        organization.avatar_url.to_string(),
                                    ),
                                )),
                            ),
                        ));
                    }
                    IssuesWebhookEventAction::Closed => {
                        container_components.push(CreateContainerComponent::Section(
                            CreateSection::new(
                                vec![
                                    CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                                        "## <:hammer:1150889684227076227> Issue closed",
                                    )),
                                    CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                                        format!(
                                            "[**{}**]({}) closed an issue:",
                                            sender.login, sender.html_url,
                                        ),
                                    )),
                                    CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                                        format!(
                                            "[`#{} {}`]({})",
                                            issue.issue.number,
                                            issue.issue.title,
                                            issue.issue.html_url
                                        ),
                                    )),
                                ],
                                CreateSectionAccessory::Thumbnail(CreateThumbnail::new(
                                    CreateUnfurledMediaItem::new(
                                        organization.avatar_url.to_string(),
                                    ),
                                )),
                            ),
                        ));
                    }
                    _ => {}
                }
            }
            WebhookEventPayload::WorkflowJob(workflow_job) => {
                #[derive(Deserialize)]
                struct WorkflowJobData {
                    id: i64,
                    run_id: i64,
                    name: String,
                    head_sha: String,
                    status: octocrab::models::workflows::Status,
                }

                let workflow_job_data: WorkflowJobData =
                    serde_json::from_value(workflow_job.workflow_job)?;

                let _lock = MESSAGE_LOCK.lock().await;
                let mut github_message: crate::models::GithubMessage = sqlx::query_as(
                    "SELECT * FROM github_messages WHERE repository_id = ? AND workflow_sha = ?",
                )
                .bind(*repository.id as i64)
                .bind(workflow_job_data.head_sha)
                .fetch_one(state.database.read())
                .await?;

                github_message
                    .workflow_status
                    .entry(workflow_job_data.id)
                    .or_insert_with(|| crate::models::WorkflowStatus {
                        name: workflow_job_data.name,
                        status: octocrab::models::workflows::Status::Queued,
                        started: chrono::Utc::now(),
                    })
                    .status = workflow_job_data.status;

                sqlx::query("UPDATE github_messages SET workflow_status = ? WHERE id = ?")
                    .bind(serde_json::to_string(&github_message.workflow_status)?)
                    .bind(github_message.id)
                    .execute(state.database.write())
                    .await?;

                edit_github_message = Some((github_message, workflow_job_data.run_id));
            }
            _ => {
                return ApiResponse::json(Response {}).ok();
            }
        };

        let Some(channel) = state
            .bot
            .read()
            .await
            .get_channel(state.env.github_channel_id.into())
            .await?
            .guild()
        else {
            tracing::error!(
                "github webhook channel ID {} is not a guild channel",
                state.env.github_channel_id
            );
            return ApiResponse::json(Response {}).ok();
        };

        if let Some((edit_github_message, run_id)) = edit_github_message {
            let mut commit_string = String::new();

            for commit in edit_github_message.commits.iter() {
                commit_string.push_str(&format!(
                    "[`{}`]({}): {}\n",
                    commit.id.chars().take(7).collect::<String>(),
                    commit.url,
                    commit.message.lines().next().unwrap_or_default()
                ));
            }

            container_components.push(CreateContainerComponent::Section(CreateSection::new(
                vec![
                    CreateSectionComponent::TextDisplay(CreateTextDisplay::new(format!(
                        "## <:package:1150890021516234832> {} Commit{} pushed",
                        edit_github_message.commits.len(),
                        if edit_github_message.commits.len() == 1 {
                            ""
                        } else {
                            "s"
                        }
                    ))),
                    CreateSectionComponent::TextDisplay(CreateTextDisplay::new(commit_string)),
                ],
                CreateSectionAccessory::Thumbnail(CreateThumbnail::new(
                    CreateUnfurledMediaItem::new(organization.avatar_url.to_string()),
                )),
            )));
            container_components.push(CreateContainerComponent::Separator(CreateSeparator::new(
                true,
            )));

            let mut workflow_status_string = String::new();

            for workflow_status in edit_github_message.workflow_status.values() {
                workflow_status_string.push_str(&format!(
                    "{} **{}** <t:{}:R>\n",
                    match workflow_status.status {
                        octocrab::models::workflows::Status::Completed =>
                            "<:accept:1156939740654878750>",
                        octocrab::models::workflows::Status::InProgress =>
                            "<a:loading:1154135013948915793>",
                        octocrab::models::workflows::Status::Failed =>
                            "<:deny:1156939743230173234>",
                        _ => "<:clock:1150889651914158111>",
                    },
                    workflow_status.name,
                    workflow_status.started.timestamp()
                ));
            }

            container_components.push(CreateContainerComponent::Section(CreateSection::new(
                vec![
                    CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                        "### Workflow Status",
                    )),
                    CreateSectionComponent::TextDisplay(CreateTextDisplay::new(
                        workflow_status_string,
                    )),
                ],
                CreateSectionAccessory::Button(
                    CreateButton::new_link(format!(
                        "{}/actions/runs/{}",
                        repository
                            .html_url
                            .as_ref()
                            .map_or_else(|| "".to_string(), |h| h.to_string()),
                        run_id
                    ))
                    .label("View Action"),
                ),
            )));

            let mut message = state
                .bot
                .read()
                .await
                .get_message(
                    channel.id.into(),
                    (edit_github_message.message_id as u64).into(),
                )
                .await?;

            container_components.push(CreateContainerComponent::TextDisplay(
                CreateTextDisplay::new(format!(
                    "-# {}",
                    repository
                        .html_url
                        .map_or_else(|| repository.name, |h| h.to_string())
                )),
            ));
            let component = CreateComponent::Container(CreateContainer::new(container_components));

            message
                .edit(
                    &*state.bot.read().await,
                    serenity::all::EditMessage::new()
                        .components(&[component])
                        .flags(MessageFlags::IS_COMPONENTS_V2),
                )
                .await?;

            return ApiResponse::json(Response {}).ok();
        } else {
            container_components.push(CreateContainerComponent::TextDisplay(
                CreateTextDisplay::new(format!(
                    "-# {}",
                    repository
                        .html_url
                        .map_or_else(|| repository.name, |h| h.to_string())
                )),
            ));
            let component = CreateComponent::Container(CreateContainer::new(container_components));

            let message = channel
                .send_message(
                    &*state.bot.read().await,
                    CreateMessage::new()
                        .components(&[component])
                        .flags(MessageFlags::IS_COMPONENTS_V2),
                )
                .await?;

            if let Some((head_commit, commits)) = create_github_message {
                sqlx::query("INSERT INTO github_messages (repository_id, message_id, commits, workflow_sha, workflow_status) VALUES (?, ?, ?, ?, ?)")
                    .bind(*repository.id as i64)
                    .bind(message.id.get() as i64)
                    .bind(serde_json::to_string(&commits)?)
                    .bind(head_commit.id)
                    .bind("{}")
                    .execute(state.database.write())
                    .await?;
            }
        }

        ApiResponse::json(Response {}).ok()
    }
}

pub fn router(state: &State) -> OpenApiRouter<State> {
    OpenApiRouter::new()
        .routes(routes!(post::route))
        .with_state(state.clone())
}
