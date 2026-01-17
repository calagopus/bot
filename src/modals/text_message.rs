use poise::Modal;

#[derive(Debug, Modal)]
#[name = "Send Text Message Once"]
pub struct TextMessageModal {
    #[name = "Text Message Title"]
    #[min_length = 1]
    #[max_length = 128]
    pub title: String,
    #[name = "Text Message Content"]
    #[min_length = 1]
    #[paragraph]
    pub content: String,
}
