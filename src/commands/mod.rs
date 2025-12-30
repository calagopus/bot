mod admin;
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
        .add_command(admin::admin_command())
}
