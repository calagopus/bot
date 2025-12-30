use futures::TryStreamExt;
use serenity::all::CreateAutocompleteResponse;
use sqlx::{FromRow, Row, sqlite::SqliteRow};

#[derive(Debug)]
pub struct TextMessage {
    pub id: i64,
    pub channel_id: i64,
    pub message_id: Option<i64>,

    pub title: String,
    pub content: String,

    pub roles: indexmap::IndexMap<u64, String>,
}

impl FromRow<'_, SqliteRow> for TextMessage {
    fn from_row(row: &SqliteRow) -> sqlx::Result<Self> {
        Ok(Self {
            id: row.try_get("id")?,
            channel_id: row.try_get("channel_id")?,
            message_id: row.try_get("message_id")?,
            title: row.try_get("title")?,
            content: row.try_get("content")?,
            roles: serde_json::from_str(&row.try_get::<String, _>("roles")?).map_err(|e| {
                sqlx::Error::ColumnDecode {
                    index: "roles".into(),
                    source: Box::new(e),
                }
            })?,
        })
    }
}

impl TextMessage {
    pub async fn get_message(
        &self,
        http: &serenity::http::Http,
    ) -> Result<(serenity::all::GuildChannel, Option<serenity::all::Message>), anyhow::Error> {
        let channel = http.get_channel((self.channel_id as u64).into()).await?;

        let channel = channel.guild().ok_or_else(|| {
            anyhow::anyhow!("channel with ID {} is not a guild channel", self.channel_id)
        })?;

        if let Some(message_id) = self.message_id {
            let message = http
                .get_message(channel.id.into(), (message_id as u64).into())
                .await
                .ok();

            Ok((channel, message))
        } else {
            Ok((channel, None))
        }
    }

    pub async fn send_or_update(
        &mut self,
        http: &serenity::http::Http,
        database: &crate::database::Database,
    ) -> Result<serenity::all::Message, anyhow::Error> {
        let (channel, existing_message) = self.get_message(http).await?;

        if let Some(mut message) = existing_message {
            message
                .edit(
                    http,
                    serenity::all::EditMessage::new()
                        .components(&[self.get_component()])
                        .flags(serenity::all::MessageFlags::IS_COMPONENTS_V2),
                )
                .await?;

            Ok(message)
        } else {
            let message = channel
                .send_message(
                    http,
                    serenity::all::CreateMessage::new()
                        .components(&[self.get_component()])
                        .flags(serenity::all::MessageFlags::IS_COMPONENTS_V2),
                )
                .await?;

            sqlx::query("UPDATE text_messages SET message_id = ? WHERE id = ?")
                .bind(message.id.get() as i64)
                .bind(self.id)
                .execute(database.write())
                .await?;
            self.message_id = Some(message.id.get() as i64);

            Ok(message)
        }
    }

    pub fn get_component(&self) -> serenity::all::CreateComponent<'_> {
        let mut container_components = vec![
            serenity::all::CreateContainerComponent::TextDisplay(
                serenity::all::CreateTextDisplay::new(format!("## {}", self.title)),
            ),
            serenity::all::CreateContainerComponent::TextDisplay(
                serenity::all::CreateTextDisplay::new(self.content.clone()),
            ),
        ];

        if !self.roles.is_empty() {
            let mut options = Vec::new();
            for (role_id, role_name) in &self.roles {
                options.push(serenity::all::CreateSelectMenuOption::new(
                    role_name,
                    role_id.to_string(),
                ));
            }

            container_components.push(serenity::all::CreateContainerComponent::Separator(
                serenity::all::CreateSeparator::new(true),
            ));
            container_components.push(serenity::all::CreateContainerComponent::ActionRow(
                serenity::all::CreateActionRow::SelectMenu(
                    serenity::all::CreateSelectMenu::new(
                        format!("text_message_roles_select:{}", self.id),
                        serenity::all::CreateSelectMenuKind::String {
                            options: options.into(),
                        },
                    )
                    .placeholder("Select your roles")
                    .min_values(0)
                    .max_values(self.roles.len() as u8),
                ),
            ));
        }

        serenity::all::CreateComponent::Container(serenity::all::CreateContainer::new(
            container_components,
        ))
    }
}

pub async fn autocomplete_text_message_id<'a>(
    ctx: crate::BotContext<'_>,
    partial: &'a str,
) -> serenity::all::CreateAutocompleteResponse<'a> {
    let database = &ctx.data().database;
    let mut text_messages = sqlx::query_as(
        "SELECT * FROM text_messages WHERE title LIKE ? ORDER BY created DESC LIMIT 25",
    )
    .bind(format!("%{}%", partial))
    .fetch(database.read());

    let mut response = CreateAutocompleteResponse::new();

    while let Ok(Some(text_message)) = text_messages.try_next().await {
        let text_message: TextMessage = text_message;
        response = response.add_choice(serenity::all::AutocompleteChoice::new(
            text_message.title,
            text_message.id as u64,
        ));
    }

    response
}
