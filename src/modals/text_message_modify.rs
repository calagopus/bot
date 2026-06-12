use poise::Modal;
use serenity::{
    all::Role,
    small_fixed_array::{FixedArray, FixedString},
};

#[derive(Debug, Modal)]
#[name = "Configure Text Message"]
pub struct TextMessageModifyModal {
    #[name = "Text Message Title"]
    #[min_length = 1]
    #[max_length = 128]
    pub title: FixedString<u16>,
    #[name = "Text Message Content"]
    #[min_length = 1]
    #[paragraph]
    pub content: FixedString<u16>,
    #[name = "Roles"]
    #[role_select]
    #[placeholder = "Select roles that can be selected by users"]
    pub roles: Option<FixedArray<Role>>,
}
