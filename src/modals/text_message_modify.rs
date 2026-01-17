use poise::Modal;
use serenity::all::Role;

#[derive(Debug, Modal)]
#[name = "Configure Text Message"]
pub struct TextMessageModifyModal {
    #[name = "Text Message Title"]
    #[min_length = 1]
    #[max_length = 128]
    pub title: String,
    #[name = "Text Message Content"]
    #[min_length = 1]
    #[paragraph]
    pub content: String,
    #[name = "Roles"]
    #[role_select()]
    #[max_items = 25]
    #[placeholder = "Select roles that can be selected by users"]
    pub roles: Option<Vec<Role>>,
}
