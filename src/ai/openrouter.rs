use serde::{Deserialize, Serialize};

pub const API_URL: &str = "https://openrouter.ai/api/v1/chat/completions";

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ImageUrl {
    pub url: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Part {
    Text { text: String },
    ImageUrl { image_url: ImageUrl },
}

impl Part {
    #[inline]
    pub fn image(mime_type: &str, data: &str) -> Self {
        Self::ImageUrl {
            image_url: ImageUrl {
                url: format!("data:{mime_type};base64,{data}"),
            },
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum Content {
    Text(String),
    Parts(Vec<Part>),
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FunctionCall {
    pub name: String,
    /// JSON encoded as a string
    pub arguments: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ToolCall {
    #[serde(default)]
    pub id: String,
    #[serde(rename = "type", default = "function_kind")]
    pub kind: String,
    pub function: FunctionCall,
}

fn function_kind() -> String {
    "function".to_string()
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Message {
    pub role: Role,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<Content>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    #[inline]
    pub fn new(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: Some(Content::Text(content.into())),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    #[inline]
    pub fn parts(role: Role, parts: Vec<Part>) -> Self {
        Self {
            role,
            content: Some(Content::Parts(parts)),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    #[inline]
    pub fn tool_result(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: Some(Content::Text(content.into())),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
        }
    }

    pub fn text(&self) -> String {
        match &self.content {
            Some(Content::Text(text)) => text.clone(),
            Some(Content::Parts(parts)) => parts
                .iter()
                .filter_map(|part| match part {
                    Part::Text { text } => Some(text.as_str()),
                    Part::ImageUrl { .. } => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
            None => String::new(),
        }
    }
}

#[derive(Serialize, Debug, Clone)]
pub struct Function<'a> {
    pub name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<&'a str>,
    pub parameters: serde_json::Value,
}

#[derive(Serialize, Debug, Clone)]
pub struct Tool<'a> {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub function: Function<'a>,
}

impl<'a> Tool<'a> {
    #[inline]
    pub fn function(
        name: &'a str,
        description: Option<&'a str>,
        parameters: serde_json::Value,
    ) -> Self {
        Self {
            kind: "function",
            function: Function {
                name,
                description,
                parameters,
            },
        }
    }
}

#[derive(Serialize, Debug, Clone)]
#[serde(untagged)]
pub enum ToolChoice<'a> {
    Mode(&'static str),
    Function {
        #[serde(rename = "type")]
        kind: &'static str,
        function: ToolChoiceFunction<'a>,
    },
}

#[derive(Serialize, Debug, Clone)]
pub struct ToolChoiceFunction<'a> {
    pub name: &'a str,
}

impl<'a> ToolChoice<'a> {
    #[inline]
    pub fn required(name: &'a str) -> Self {
        Self::Function {
            kind: "function",
            function: ToolChoiceFunction { name },
        }
    }
}

#[derive(Serialize, Debug)]
pub struct Request<'a> {
    pub model: &'a str,
    pub messages: &'a [Message],
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    pub tools: &'a [Tool<'a>],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
}

#[derive(Deserialize, Debug)]
struct Choice {
    message: Message,
}

#[derive(Deserialize, Debug)]
struct Response {
    #[serde(default)]
    choices: Vec<Choice>,
    #[serde(default)]
    error: Option<serde_json::Value>,
}

pub async fn complete(
    env: &crate::env::Env,
    request: Request<'_>,
) -> Result<Message, anyhow::Error> {
    let api_key = env
        .openrouter_api_key
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("OPENROUTER_API_KEY is not set"))?;

    let response = super::CLIENT
        .post(API_URL)
        .bearer_auth(api_key)
        .header("HTTP-Referer", &env.app_url)
        .header("X-Title", "Calagopus Bot")
        .json(&request)
        .send()
        .await?;

    let status = response.status();
    let body = response.text().await?;

    if !status.is_success() {
        return Err(anyhow::anyhow!(
            "openrouter request failed with status {status}: {}",
            body.chars().take(512).collect::<String>()
        ));
    }

    let response: Response = serde_json::from_str(&body)?;

    if let Some(error) = response.error {
        return Err(anyhow::anyhow!("openrouter returned an error: {error}"));
    }

    response
        .choices
        .into_iter()
        .next()
        .map(|choice| choice.message)
        .ok_or_else(|| anyhow::anyhow!("openrouter returned no choices"))
}
