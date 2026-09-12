use super::extension;

const MIN_QUOTE_LENGTH: usize = 24;
const MAX_REPLY_IMAGES: usize = 4;

fn normalize(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[derive(Debug)]
pub struct Source {
    pub page: String,
    pub quote: String,
}

#[derive(Default, Debug)]
pub struct Retrieved {
    texts: Vec<String>,
}

impl Retrieved {
    pub fn push(&mut self, text: &str) {
        self.texts.push(normalize(text));
    }

    pub fn verifies(&self, quote: &str) -> bool {
        let quote = normalize(quote);

        if quote.len() < MIN_QUOTE_LENGTH {
            return false;
        }

        self.texts.iter().any(|text| text.contains(&quote))
    }
}

pub struct Answer {
    pub text: String,
    pub sources: Vec<Source>,
    pub images: Vec<String>,
    /// Sources the model gave whose quote could not be found in what the tools returned.
    pub dropped: usize,
}

fn is_documentation_image(name: &str) -> bool {
    const TYPES: [&str; 4] = ["webp", "png", "jpg", "jpeg"];

    (name.starts_with('/') || name.starts_with("https://"))
        && extension(name).is_some_and(|extension| TYPES.contains(&extension.as_str()))
}

pub fn parse(arguments: &str, retrieved: &Retrieved) -> Result<Answer, anyhow::Error> {
    #[derive(serde::Deserialize)]
    struct Submitted {
        answer: String,
        #[serde(default)]
        sources: Vec<SubmittedSource>,
        #[serde(default)]
        images: Vec<String>,
    }

    #[derive(serde::Deserialize)]
    struct SubmittedSource {
        page: String,
        quote: String,
    }

    let submitted: Submitted = serde_json::from_str(arguments)?;

    let mut sources: Vec<Source> = Vec::new();
    let mut dropped = 0;

    for source in submitted.sources {
        if !retrieved.verifies(&source.quote) {
            tracing::debug!(
                page = %source.page,
                "ai support dropped an unverifiable source quote"
            );
            dropped += 1;

            continue;
        }

        if sources.iter().any(|existing| existing.page == source.page) {
            continue;
        }

        sources.push(Source {
            page: source.page,
            quote: source.quote,
        });
    }

    Ok(Answer {
        text: submitted.answer,
        sources,
        images: submitted
            .images
            .into_iter()
            .filter(|image| is_documentation_image(image))
            .take(MAX_REPLY_IMAGES)
            .collect(),
        dropped,
    })
}
