mod admin;
pub mod sponsor;
mod status;

pub struct CommandList {
    commands: Vec<poise::Command<crate::routes::InnerState, anyhow::Error>>,
}

impl CommandList {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    pub fn add_command(
        mut self,
        command: poise::Command<crate::routes::InnerState, anyhow::Error>,
    ) -> Self {
        self.commands.push(command);

        self
    }

    pub fn into_vec(self) -> Vec<poise::Command<crate::routes::InnerState, anyhow::Error>> {
        self.commands
    }
}

pub fn commands(list: CommandList) -> CommandList {
    list.add_command(status::status_command())
        .add_command(sponsor::sponsor_command())
        .add_command(admin::admin_command())
}

fn field(value: &serde_json::Value, key: &str, default: serde_json::Value) -> serde_json::Value {
    match value.get(key) {
        Some(serde_json::Value::Null) | None => default,
        Some(value) => value.clone(),
    }
}

fn normalize_option(value: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "type": field(value, "type", serde_json::Value::Null),
        "name": field(value, "name", serde_json::Value::Null),
        "name_localizations": field(value, "name_localizations", serde_json::Value::Null),
        "description": field(value, "description", serde_json::json!("")),
        "description_localizations": field(value, "description_localizations", serde_json::Value::Null),
        "required": field(value, "required", serde_json::json!(false)),
        "autocomplete": field(value, "autocomplete", serde_json::json!(false)),
        "channel_types": field(value, "channel_types", serde_json::json!([])),
        "min_value": field(value, "min_value", serde_json::Value::Null),
        "max_value": field(value, "max_value", serde_json::Value::Null),
        "min_length": field(value, "min_length", serde_json::Value::Null),
        "max_length": field(value, "max_length", serde_json::Value::Null),
        "choices": field(value, "choices", serde_json::json!([]))
            .as_array()
            .map(|choices| {
                choices
                    .iter()
                    .map(|choice| {
                        serde_json::json!({
                            "name": field(choice, "name", serde_json::Value::Null),
                            "name_localizations": field(choice, "name_localizations", serde_json::Value::Null),
                            "value": field(choice, "value", serde_json::Value::Null),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
        "options": field(value, "options", serde_json::json!([]))
            .as_array()
            .map(|options| options.iter().map(normalize_option).collect::<Vec<_>>())
            .unwrap_or_default(),
    })
}

fn normalize_command(value: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "type": field(value, "type", serde_json::json!(1)),
        "name": field(value, "name", serde_json::Value::Null),
        "name_localizations": field(value, "name_localizations", serde_json::Value::Null),
        "description": field(value, "description", serde_json::json!("")),
        "description_localizations": field(value, "description_localizations", serde_json::Value::Null),
        "default_member_permissions": field(value, "default_member_permissions", serde_json::Value::Null),
        "nsfw": field(value, "nsfw", serde_json::json!(false)),
        "contexts": field(value, "contexts", serde_json::Value::Null),
        "integration_types": field(value, "integration_types", serde_json::Value::Null),
        "options": field(value, "options", serde_json::json!([]))
            .as_array()
            .map(|options| options.iter().map(normalize_option).collect::<Vec<_>>())
            .unwrap_or_default(),
    })
}

fn command_key(value: &serde_json::Value) -> (String, u64) {
    (
        value["name"].as_str().unwrap_or_default().to_string(),
        value["type"].as_u64().unwrap_or(1),
    )
}

fn command_matches(local: &serde_json::Value, remote: &serde_json::Value) -> bool {
    let mut remote = remote.clone();

    for key in ["contexts", "integration_types"] {
        if local[key].is_null() {
            remote[key] = serde_json::Value::Null;
        }
    }

    *local == remote
}

fn commands_match(
    local: &[serenity::all::CreateCommand<'_>],
    remote: &[serenity::all::Command],
) -> Result<bool, anyhow::Error> {
    if local.len() != remote.len() {
        return Ok(false);
    }

    let mut remote_commands = std::collections::HashMap::with_capacity(remote.len());
    for command in remote {
        let command = normalize_command(&serde_json::to_value(command)?);
        remote_commands.insert(command_key(&command), command);
    }

    for command in local {
        let command = normalize_command(&serde_json::to_value(command)?);

        match remote_commands.get(&command_key(&command)) {
            Some(remote) if command_matches(&command, remote) => {}
            _ => return Ok(false),
        }
    }

    Ok(true)
}

pub async fn register_commands(http: &serenity::all::Http) -> Result<(), anyhow::Error> {
    let local = poise::builtins::create_application_commands(
        commands(CommandList::new()).into_vec().iter(),
    );
    let remote = serenity::all::Command::get_global_commands_with_localizations(http).await?;

    if commands_match(&local, &remote)? {
        tracing::info!(count = local.len(), "bot commands are up to date");

        return Ok(());
    }

    serenity::all::Command::set_global_commands(http, &local).await?;

    tracing::info!(count = local.len(), "bot commands registered");

    Ok(())
}
