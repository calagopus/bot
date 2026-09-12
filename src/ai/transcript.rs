use super::extension;

/// Largest attachment downloaded at all, and how much of one is kept once it is.
const MAX_ATTACHMENT_BYTES: u32 = 1024 * 1024;
const ATTACHMENT_HEAD: usize = 2000;
const ATTACHMENT_TAIL: usize = 6000;
/// Attachment text inlined into one transcript, across every message in it.
const MAX_ATTACHMENT_TEXT: usize = 96 * 1024;
/// Files read from one thread, so a post full of uploads cannot stall an answer.
const MAX_ATTACHMENT_FILES: usize = 6;
const ATTACHMENT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
/// Screenshots from a thread handed to the model, newest kept when there are more.
const MAX_THREAD_IMAGES: usize = 4;
const MAX_IMAGE_BYTES: u32 = 8 * 1024 * 1024;

fn is_text_attachment(attachment: &serenity::all::Attachment) -> bool {
    const EXTENSIONS: [&str; 16] = [
        "log",
        "txt",
        "yml",
        "yaml",
        "json",
        "conf",
        "cfg",
        "ini",
        "toml",
        "properties",
        "env",
        "sh",
        "xml",
        "md",
        "csv",
        "list",
    ];

    if extension(&attachment.filename)
        .is_some_and(|extension| EXTENSIONS.contains(&extension.as_str()))
    {
        return true;
    }

    attachment
        .content_type
        .as_deref()
        .is_some_and(|content_type| {
            let content_type = content_type.to_lowercase();

            content_type.starts_with("text/")
                || content_type.contains("json")
                || content_type.contains("yaml")
                || content_type.contains("xml")
        })
}

fn image_mime_type(attachment: &serenity::all::Attachment) -> Option<&'static str> {
    const TYPES: [(&str, &str); 5] = [
        ("png", "image/png"),
        ("jpg", "image/jpeg"),
        ("jpeg", "image/jpeg"),
        ("webp", "image/webp"),
        ("gif", "image/gif"),
    ];

    let extension = extension(&attachment.filename)?;

    TYPES
        .into_iter()
        .find(|(candidate, _)| *candidate == extension)
        .map(|(_, mime_type)| mime_type)
}

pub struct Image {
    pub filename: String,
    pub mime_type: &'static str,
    pub data: String,
}

async fn attachment_image(attachment: &serenity::all::Attachment) -> Option<Image> {
    let mime_type = image_mime_type(attachment)?;

    if attachment.size > MAX_IMAGE_BYTES
        || attachment
            .content_type
            .as_deref()
            .is_some_and(|content_type| !content_type.starts_with("image/"))
    {
        return None;
    }

    let bytes = match super::CLIENT
        .get(attachment.url.as_str())
        .timeout(ATTACHMENT_TIMEOUT)
        .send()
        .await
    {
        Ok(response) => response.bytes().await.ok()?,
        Err(err) => {
            tracing::debug!(
                file = %attachment.filename,
                "ai support could not read an image: {err}"
            );

            return None;
        }
    };

    Some(Image {
        filename: attachment.filename.to_string(),
        mime_type,
        data: base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes),
    })
}

fn longest_backtick_run(text: &str) -> usize {
    let mut longest = 0;
    let mut run = 0;

    for character in text.chars() {
        if character == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }

    longest
}

fn clamp_attachment(text: &str) -> String {
    let characters = text.chars().count();

    if characters <= ATTACHMENT_HEAD + ATTACHMENT_TAIL {
        return text.to_string();
    }

    let head = text.chars().take(ATTACHMENT_HEAD).collect::<String>();
    let tail = text
        .chars()
        .skip(characters - ATTACHMENT_TAIL)
        .collect::<String>();

    format!(
        "{head}\n\n[... {} characters omitted ...]\n\n{tail}",
        characters - ATTACHMENT_HEAD - ATTACHMENT_TAIL
    )
}

async fn attachment_text(attachment: &serenity::all::Attachment, budget: usize) -> Option<String> {
    if !is_text_attachment(attachment) || attachment.size > MAX_ATTACHMENT_BYTES {
        return None;
    }

    let response = super::CLIENT.get(attachment.url.as_str()).send().await;
    let text = match response {
        Ok(response) => response.text().await.ok()?,
        Err(err) => {
            tracing::debug!(
                file = %attachment.filename,
                "ai support could not read an attachment: {err}"
            );

            return None;
        }
    };

    let text = clamp_attachment(&text);

    if text.chars().count() > budget {
        return None;
    }

    Some(text)
}

pub struct Context {
    pub text: String,
    pub images: Vec<Image>,
}

pub async fn transcript(
    bot_id: serenity::all::UserId,
    title: &str,
    messages: &[serenity::all::Message],
) -> Context {
    let mut transcript = format!("Forum post title: {title}\n\n");
    let mut budget = MAX_ATTACHMENT_TEXT;
    let mut files = 0;
    let mut posted_images = Vec::new();

    for message in messages {
        let bot = message.author.id == bot_id;

        let author = if bot {
            "support bot".to_string()
        } else {
            message
                .author
                .name
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        };

        let mut content = message.content.to_string();

        if !bot {
            for attachment in message.attachments.iter() {
                if image_mime_type(attachment).is_some() {
                    content.push_str(&format!("\n[image {}]", attachment.filename));
                    posted_images.push(attachment);

                    continue;
                }

                if files >= MAX_ATTACHMENT_FILES {
                    content.push_str(&format!("\n[file {}, not read]", attachment.filename));

                    continue;
                }

                match attachment_text(attachment, budget).await {
                    Some(text) => {
                        files += 1;
                        budget = budget.saturating_sub(text.len());
                        let fence = "`".repeat(longest_backtick_run(&text).max(2) + 1);

                        content.push_str(&format!(
                            "\n[file {}]\n{fence}\n{}\n{fence}",
                            attachment.filename, text
                        ));
                    }
                    None => {
                        content.push_str(&format!("\n[file {}, not read]", attachment.filename))
                    }
                }
            }
        }

        if content.trim().is_empty() {
            continue;
        }

        transcript.push_str(&format!("{author}: {content}\n\n"));
    }

    if posted_images.len() > MAX_THREAD_IMAGES {
        posted_images.drain(..posted_images.len() - MAX_THREAD_IMAGES);
    }

    let mut images = Vec::new();
    for attachment in posted_images {
        if let Some(image) = attachment_image(attachment).await {
            images.push(image);
        }
    }

    Context {
        text: transcript,
        images,
    }
}
