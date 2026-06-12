use poise::Modal;
use serenity::small_fixed_array::FixedString;

#[derive(Debug, Modal)]
#[name = "Send Text Message Once"]
pub struct TextMessageModal {
    #[name = "Text Message Title"]
    #[min_length = 1]
    #[max_length = 128]
    pub title: FixedString<u16>,
    #[name = "Text Message Content"]
    #[min_length = 1]
    #[paragraph]
    pub content: FixedString<u16>,
}
