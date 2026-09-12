use rmcp::{
    ServiceExt,
    model::{
        CallToolRequest, CallToolRequestParams, ClientCapabilities, ClientInfo, ClientRequest,
        ContentBlock, Implementation, ServerResult,
    },
    service::{ClientCacheConfig, RoleClient, RunningService, ServiceError},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use std::sync::Arc;

const TOOL_CACHE_TTL: std::time::Duration = std::time::Duration::from_mins(30);

type Service = RunningService<RoleClient, ClientInfo>;

#[derive(Debug, Clone)]
pub struct Tool {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Clone)]
pub enum Content {
    Text(String),
    Image { mime_type: String, data: String },
}

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub content: Vec<Content>,
    pub is_error: bool,
}

impl ToolResult {
    pub fn text(&self) -> String {
        self.content
            .iter()
            .map(|content| match content {
                Content::Text(value) => value.clone(),
                Content::Image { mime_type, .. } => {
                    format!("[{mime_type} attached to the next message]")
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[inline]
    pub fn images(&self) -> impl Iterator<Item = (&str, &str)> {
        self.content.iter().filter_map(|content| match content {
            Content::Image { mime_type, data } => Some((mime_type.as_str(), data.as_str())),
            _ => None,
        })
    }
}

#[derive(Debug)]
pub struct Client {
    url: String,
    service: tokio::sync::RwLock<Option<Arc<Service>>>,
}

impl Client {
    #[inline]
    pub fn new(url: String) -> Self {
        Self {
            url,
            service: tokio::sync::RwLock::new(None),
        }
    }

    async fn connect(&self) -> Result<Service, anyhow::Error> {
        let transport = StreamableHttpClientTransport::with_client(
            super::CLIENT.clone(),
            StreamableHttpClientTransportConfig::with_uri(self.url.clone()),
        );

        let service = ClientInfo::new(
            ClientCapabilities::default(),
            Implementation::new("calagopus-bot", crate::VERSION),
        )
        .serve(transport)
        .await?;

        service
            .peer()
            .set_response_cache_config(
                ClientCacheConfig::default().with_default_ttl(TOOL_CACHE_TTL),
            )
            .await;

        Ok(service)
    }

    async fn service(&self) -> Result<Arc<Service>, anyhow::Error> {
        if let Some(service) = self.service.read().await.clone() {
            return Ok(service);
        }

        let mut slot = self.service.write().await;

        if let Some(service) = slot.clone() {
            return Ok(service);
        }

        let service = Arc::new(self.connect().await?);
        *slot = Some(service.clone());

        Ok(service)
    }

    async fn discard(&self, dead: &Arc<Service>) {
        let mut slot = self.service.write().await;

        if slot.as_ref().is_some_and(|live| Arc::ptr_eq(live, dead)) {
            slot.take();
        }
    }

    async fn retrying<T, F: Future<Output = Result<T, ServiceError>>>(
        &self,
        call: impl Fn(Arc<Service>) -> F,
    ) -> Result<T, anyhow::Error> {
        let service = self.service().await?;

        match call(service.clone()).await {
            Err(ServiceError::TransportClosed) => {}
            result => return Ok(result?),
        }

        self.discard(&service).await;

        Ok(call(self.service().await?).await?)
    }

    pub async fn tools(&self) -> Result<Vec<Tool>, anyhow::Error> {
        let tools = self
            .retrying(|service: Arc<Service>| async move { service.peer().list_all_tools().await })
            .await?;

        Ok(tools
            .into_iter()
            .map(|tool| Tool {
                name: tool.name.into_owned(),
                description: tool.description.map(|description| description.into_owned()),
                input_schema: serde_json::Value::Object((*tool.input_schema).clone()),
            })
            .collect())
    }

    pub async fn call(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<ToolResult, anyhow::Error> {
        let mut params = CallToolRequestParams::new(name.to_string());

        if let serde_json::Value::Object(arguments) = arguments {
            params = params.with_arguments(arguments);
        }

        let result = self
            .retrying(|service: Arc<Service>| {
                let params = params.clone();

                async move {
                    service
                        .peer()
                        .send_request(ClientRequest::CallToolRequest(CallToolRequest::new(params)))
                        .await
                }
            })
            .await?;

        Ok(tool_result(result))
    }
}

fn tool_result(result: ServerResult) -> ToolResult {
    match result {
        ServerResult::CallToolResult(result) => {
            let mut content = result.content.iter().map(content_block).collect::<Vec<_>>();

            if content.is_empty()
                && let Some(structured) = result.structured_content
            {
                content.push(Content::Text(structured.to_string()));
            }

            ToolResult {
                content,
                is_error: result.is_error.unwrap_or(false),
            }
        }
        ServerResult::CustomResult(result) => lenient(&result.0),
        _ => ToolResult {
            content: Vec::new(),
            is_error: false,
        },
    }
}

fn content_block(block: &ContentBlock) -> Content {
    match block {
        ContentBlock::Text(text) => Content::Text(text.text.clone()),
        ContentBlock::Image(image) => Content::Image {
            mime_type: image.mime_type.clone(),
            data: image.data.clone(),
        },
        other => Content::Text(
            serde_json::to_value(other)
                .map(|value| value.to_string())
                .unwrap_or_default(),
        ),
    }
}

fn lenient(result: &serde_json::Value) -> ToolResult {
    let mut content = result
        .get("content")
        .and_then(|content| content.as_array())
        .map(|content| content.as_slice())
        .unwrap_or_default()
        .iter()
        .map(
            |item| match item.get("type").and_then(|kind| kind.as_str()) {
                Some("text") => Content::Text(string(item, "text").unwrap_or_default()),
                Some("image") => Content::Image {
                    mime_type: string(item, "mimeType").unwrap_or_else(|| "image/png".to_string()),
                    data: string(item, "data").unwrap_or_default(),
                },
                _ => Content::Text(item.to_string()),
            },
        )
        .collect::<Vec<_>>();

    if content.is_empty()
        && let Some(structured) = result.get("structuredContent")
    {
        content.push(Content::Text(structured.to_string()));
    }

    ToolResult {
        content,
        is_error: result
            .get("isError")
            .and_then(|is_error| is_error.as_bool())
            .unwrap_or(false),
    }
}

fn string(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[track_caller]
    fn result(raw: &str) -> ToolResult {
        tool_result(serde_json::from_str(raw).expect("result did not parse as any server result"))
    }

    #[test]
    fn reads_an_ordinary_result() {
        let result = result(r#"{"content":[{"type":"text","text":"hello"}]}"#);

        assert_eq!(result.text(), "hello");
        assert!(!result.is_error);
    }

    /// A server that says nothing about failure has not failed. Reading the missing field as an
    /// error would keep every page out of the corpus the answer's quotes are checked against, and
    /// every answer would post as unsourced.
    #[test]
    fn a_missing_error_flag_is_not_an_error() {
        assert!(!result(r#"{"content":[{"type":"text","text":"no such page"}]}"#).is_error);
        assert!(result(r#"{"content":[{"type":"text","text":"boom"}],"isError":true}"#).is_error);
    }

    /// The typed model rejects these, so they arrive as raw JSON instead of as a failed call.
    /// Reading them the lenient way is what keeps a malformed block from costing the whole answer.
    #[test]
    fn fills_in_what_a_malformed_result_leaves_out() {
        // A text block with no text at all.
        assert_eq!(result(r#"{"content":[{"type":"text"}]}"#).text(), "");

        // An image with no mime type still counts as an image, and is offered as a png.
        let image = result(r#"{"content":[{"type":"image","data":"abc"}]}"#);
        assert_eq!(
            image.images().collect::<Vec<_>>(),
            vec![("image/png", "abc")]
        );
        assert_eq!(image.text(), "[image/png attached to the next message]");

        // A kind this SDK has never heard of reaches the model as what the server sent.
        let unknown = result(r#"{"content":[{"type":"brand_new_kind","x":1}]}"#);
        assert!(unknown.text().contains("brand_new_kind"));
        assert!(!unknown.is_error);
    }

    #[test]
    fn falls_back_to_structured_content_only_when_there_is_nothing_else() {
        assert_eq!(
            result(r#"{"structuredContent":{"a":1}}"#).text(),
            r#"{"a":1}"#
        );
        assert_eq!(
            result(r#"{"content":[{"type":"text","text":"real"}],"structuredContent":{"a":1}}"#)
                .text(),
            "real"
        );
    }

    #[test]
    fn an_empty_result_is_empty_rather_than_an_error() {
        let result = result("{}");

        assert!(result.text().is_empty());
        assert!(!result.is_error);
    }
}
