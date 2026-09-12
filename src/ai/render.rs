use super::answer::Answer;

const MAX_ANSWER_LENGTH: usize = 5000;
const MAX_SOURCES: usize = 3;

const MAX_QUOTE_LENGTH: usize = 140;
const MAX_MESSAGE_LENGTH: usize = 2000;
const MAX_MESSAGES: usize = 4;
const MAX_REPLY_ATTACHMENT_BYTES: usize = 7 * 1024 * 1024;

fn page_url(docs_url: &str, page: &str) -> String {
    if page.starts_with("http://") || page.starts_with("https://") {
        return page.to_string();
    }

    format!(
        "{}/{}",
        docs_url.trim_end_matches('/'),
        page.trim_start_matches('/')
    )
}

pub async fn documentation_images<'a>(
    env: &crate::env::Env,
    names: &[String],
) -> Vec<serenity::all::CreateAttachment<'a>> {
    if names.is_empty() {
        return Vec::new();
    }

    let result = match super::mcp_client(env)
        .call("get_images", serde_json::json!({ "names": names }))
        .await
    {
        Ok(result) => result,
        Err(err) => {
            tracing::warn!("ai support could not read documentation images: {err:?}");

            return Vec::new();
        }
    };

    let mut attachments = Vec::new();
    let mut total = 0;

    for (index, (mime_type, data)) in result.images().enumerate() {
        let Ok(bytes) = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, data)
        else {
            continue;
        };

        total += bytes.len();
        if total > MAX_REPLY_ATTACHMENT_BYTES {
            break;
        }

        let filename = names
            .get(index)
            .and_then(|name| name.rsplit('/').next())
            .map(|filename| filename.to_string())
            .unwrap_or_else(|| format!("screenshot.{}", mime_type.trim_start_matches("image/")));

        attachments.push(serenity::all::CreateAttachment::bytes(bytes, filename));
    }

    attachments
}

fn plain_quote(quote: &str) -> String {
    quote
        .replace('`', "")
        .split_whitespace()
        .filter(|word| !word.chars().all(|character| character == '#') && !word.starts_with(":::"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn clamp(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }

    format!("{}…", text.chars().take(limit).collect::<String>())
}

pub fn render(docs_url: &str, answer: &Answer) -> String {
    let mut content = clamp(answer.text.trim(), MAX_ANSWER_LENGTH);

    if answer.sources.is_empty() {
        content.push_str("\n\n-# AI answer, nothing in the docs to back it. Treat it as a guess.");

        return content;
    }

    content.push('\n');
    for source in answer.sources.iter().take(MAX_SOURCES) {
        content.push_str(&format!(
            "\n-# [{}]({}) - \"{}\"",
            source.page,
            page_url(docs_url, &source.page),
            clamp(&plain_quote(&source.quote), MAX_QUOTE_LENGTH)
        ));
    }

    content.push_str("\n-# AI answer, quotes checked word for word.");

    if answer.dropped > 0 {
        content.push_str(&format!(
            " {} unverified source(s) removed.",
            answer.dropped
        ));
    }

    content
}

pub fn split(content: &str) -> Vec<String> {
    let mut messages: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut fence: Option<String> = None;

    let flush = |current: &mut String, messages: &mut Vec<String>, fence: &Option<String>| {
        let mut message = std::mem::take(current);

        if let Some(fence) = fence {
            message.truncate(message.trim_end().len());
            message.push_str("\n```");
            messages.push(message);
            current.push_str(fence);
            current.push('\n');
        } else {
            messages.push(message);
        }
    };

    for line in content.split_inclusive('\n') {
        let mut line = line.to_string();

        while line.chars().count() > MAX_MESSAGE_LENGTH {
            let head = line.chars().take(MAX_MESSAGE_LENGTH).collect::<String>();

            let split_at = match head.rfind(char::is_whitespace) {
                Some(0) | None => head.len(),
                Some(index) => index,
            };
            let (head, tail) = head.split_at(split_at);

            if !current.is_empty() {
                flush(&mut current, &mut messages, &fence);
            }

            messages.push(head.to_string());
            line = format!("{}{}", tail, &line[head.len() + tail.len()..]);
        }

        if current.chars().count() + line.chars().count() > MAX_MESSAGE_LENGTH {
            flush(&mut current, &mut messages, &fence);
        }

        if let Some(rest) = line.trim_start().strip_prefix("```") {
            fence = match fence {
                Some(_) => None,
                None => Some(format!("```{}", rest.trim_end())),
            };
        }

        current.push_str(&line);
    }

    if !current.trim().is_empty() {
        messages.push(current);
    }

    if messages.len() > MAX_MESSAGES {
        let last = messages.pop().unwrap();
        messages.truncate(MAX_MESSAGES - 1);
        messages.push(last);
    }

    messages
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Quotes are lifted out of documentation markdown, so they arrive carrying fences, headings
    /// and container directives that a `-#` line cannot hold.
    #[test]
    fn flattens_markdown_out_of_quotes() {
        assert_eq!(
            plain_quote(
                "#### 2. Restart the service\nThe package upgrade alone doesn't restart the running daemon, do that explicitly:\n```bash\nsystemctl restart calagopus-panel\n```"
            ),
            "2. Restart the service The package upgrade alone doesn't restart the running daemon, do that explicitly: bash systemctl restart calagopus-panel"
        );

        assert_eq!(
            plain_quote(
                "::: warning This step is **mandatory before the first `docker compose up`**."
            ),
            "warning This step is **mandatory before the first docker compose up**."
        );
    }

    /// A long run with no break in it used to leave the line unchanged and spin forever, pushing
    /// an empty message on every pass.
    #[test]
    fn splits_an_unbroken_run_that_cannot_be_broken_on_a_word() {
        let content = format!(" {}", "a".repeat(MAX_MESSAGE_LENGTH + 500));
        let messages = split(&content);

        assert!(messages.len() > 1);
        assert!(messages.iter().all(|message| !message.is_empty()));
        assert!(
            messages
                .iter()
                .all(|message| message.chars().count() <= MAX_MESSAGE_LENGTH)
        );
        assert_eq!(
            messages.concat().chars().filter(|c| *c == 'a').count(),
            MAX_MESSAGE_LENGTH + 500
        );
    }
}
