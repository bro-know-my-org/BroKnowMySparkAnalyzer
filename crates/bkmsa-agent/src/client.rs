use async_trait::async_trait;
#[cfg(feature = "native-client")]
use reqwest::{header, Client};
#[cfg(feature = "native-client")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "native-client")]
use crate::{AgentError, AiConfig, ModelInfo};
use crate::{ChatMessage, Result};

#[cfg(feature = "native-client")]
const MAX_PROVIDER_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

#[cfg(not(target_arch = "wasm32"))]
#[async_trait]
pub trait ChatClient: Send + Sync {
    async fn chat(&self, messages: &[ChatMessage]) -> Result<String>;
}

#[cfg(target_arch = "wasm32")]
#[async_trait(?Send)]
pub trait ChatClient {
    async fn chat(&self, messages: &[ChatMessage]) -> Result<String>;
}

#[derive(Clone)]
#[cfg(feature = "native-client")]
pub struct OpenAiClient {
    config: AiConfig,
    http: Client,
}

#[cfg(feature = "native-client")]
impl OpenAiClient {
    pub fn new(config: AiConfig) -> Result<Self> {
        config.validate()?;
        let http = Client::builder()
            .timeout(config.timeout())
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self { config, http })
    }

    pub async fn test_connection(&self) -> Result<String> {
        self.request_chat(&[
            ChatMessage::system("You are a connectivity probe. Reply with exactly: OK"),
            ChatMessage::user("ping"),
        ])
        .await
    }

    pub async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        let response = self
            .http
            .get(self.config.endpoint("models")?)
            .bearer_auth(self.config.api_key())
            .send()
            .await?;
        let status = response.status();
        let bytes = read_limited_body(response).await?;
        if !status.is_success() {
            return Err(provider_error(status.as_u16(), &bytes));
        }
        let body: ModelsResponse = serde_json::from_slice(&bytes)?;
        Ok(body
            .data
            .into_iter()
            .map(|item| ModelInfo { id: item.id })
            .collect())
    }

    async fn request_chat(&self, messages: &[ChatMessage]) -> Result<String> {
        validate_messages(messages)?;
        let request = ChatRequest {
            model: self.config.model(),
            temperature: supports_temperature(self.config.model())
                .then_some(self.config.temperature()),
            messages,
        };
        let response = self
            .http
            .post(self.config.endpoint("chat/completions")?)
            .bearer_auth(self.config.api_key())
            .header(header::CONTENT_TYPE, "application/json")
            .json(&request)
            .send()
            .await?;
        let status = response.status();
        let bytes = read_limited_body(response).await?;
        if !status.is_success() {
            return Err(provider_error(status.as_u16(), &bytes));
        }
        let body: ChatResponse = serde_json::from_slice(&bytes)?;
        let message = body
            .choices
            .into_iter()
            .next()
            .map(|choice| choice.message)
            .ok_or(AgentError::EmptyResponse)?;
        if let Some(refusal) = message.refusal.as_deref() {
            return Err(AgentError::Refusal(sanitize_provider_text(refusal)));
        }
        message
            .text()
            .filter(|content| !content.trim().is_empty())
            .ok_or(AgentError::EmptyResponse)
    }
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg(feature = "native-client")]
impl ChatClient for OpenAiClient {
    async fn chat(&self, messages: &[ChatMessage]) -> Result<String> {
        self.request_chat(messages).await
    }
}

#[cfg(feature = "native-client")]
fn provider_error(status: u16, bytes: &[u8]) -> AgentError {
    let text = String::from_utf8_lossy(bytes);
    let message = serde_json::from_str::<ProviderErrorBody>(&text)
        .ok()
        .and_then(|body| body.error.map(|error| error.message))
        .unwrap_or_else(|| truncate_chars(&text, 1_000));
    AgentError::Provider {
        status,
        message: sanitize_provider_text(&message),
    }
}

#[cfg(feature = "native-client")]
async fn read_limited_body(mut response: reqwest::Response) -> Result<Vec<u8>> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_PROVIDER_RESPONSE_BYTES as u64)
    {
        return Err(AgentError::Provider {
            status: response.status().as_u16(),
            message: format!(
                "response exceeds {} MiB limit",
                MAX_PROVIDER_RESPONSE_BYTES / 1024 / 1024
            ),
        });
    }
    let mut bytes = Vec::with_capacity(
        response
            .content_length()
            .unwrap_or_default()
            .min(MAX_PROVIDER_RESPONSE_BYTES as u64) as usize,
    );
    while let Some(chunk) = response.chunk().await? {
        if bytes.len().saturating_add(chunk.len()) > MAX_PROVIDER_RESPONSE_BYTES {
            return Err(AgentError::Provider {
                status: response.status().as_u16(),
                message: format!(
                    "response exceeds {} MiB limit",
                    MAX_PROVIDER_RESPONSE_BYTES / 1024 / 1024
                ),
            });
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[cfg(feature = "native-client")]
fn supports_temperature(model: &str) -> bool {
    let model = model.trim().to_ascii_lowercase();
    !["gpt-5", "o1", "o3", "o4"]
        .iter()
        .any(|prefix| model == *prefix || model.starts_with(&format!("{prefix}-")))
}

#[cfg(feature = "native-client")]
fn truncate_chars(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

#[cfg(feature = "native-client")]
fn sanitize_provider_text(value: &str) -> String {
    value
        .chars()
        .take(1_000)
        .flat_map(|character| {
            if character.is_control() {
                character.escape_default().collect::<Vec<_>>()
            } else {
                vec![character]
            }
        })
        .collect()
}

#[cfg(feature = "native-client")]
fn validate_messages(messages: &[ChatMessage]) -> Result<()> {
    if messages.len() > 128
        || messages
            .iter()
            .map(|message| message.content.len())
            .sum::<usize>()
            > 2 * 1024 * 1024
    {
        return Err(AgentError::InvalidConfig(
            "provider request exceeds the message count or 2 MiB content limit".into(),
        ));
    }
    Ok(())
}

#[derive(Serialize)]
#[cfg(feature = "native-client")]
struct ChatRequest<'a> {
    model: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    messages: &'a [ChatMessage],
}

#[derive(Deserialize)]
#[cfg(feature = "native-client")]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
#[cfg(feature = "native-client")]
struct ChatChoice {
    message: ChatResponseMessage,
}

#[derive(Deserialize)]
#[cfg(feature = "native-client")]
struct ChatResponseMessage {
    content: Option<AssistantContent>,
    refusal: Option<String>,
}

#[derive(Deserialize)]
#[cfg(feature = "native-client")]
#[serde(untagged)]
enum AssistantContent {
    Text(String),
    Parts(Vec<AssistantContentPart>),
}

#[derive(Deserialize)]
#[cfg(feature = "native-client")]
struct AssistantContentPart {
    #[serde(rename = "type")]
    kind: Option<String>,
    text: Option<String>,
}

#[cfg(feature = "native-client")]
impl ChatResponseMessage {
    fn text(self) -> Option<String> {
        let content = match self.content {
            Some(AssistantContent::Text(text)) => Some(text),
            Some(AssistantContent::Parts(parts)) => {
                let text = parts
                    .into_iter()
                    .filter(|part| part.kind.as_deref().is_none_or(|kind| kind == "text"))
                    .filter_map(|part| part.text)
                    .collect::<Vec<_>>()
                    .join("");
                (!text.is_empty()).then_some(text)
            }
            None => None,
        };
        content
    }
}

#[derive(Deserialize)]
#[cfg(feature = "native-client")]
struct ModelsResponse {
    data: Vec<ModelResponseItem>,
}

#[derive(Deserialize)]
#[cfg(feature = "native-client")]
struct ModelResponseItem {
    id: String,
}

#[derive(Deserialize)]
#[cfg(feature = "native-client")]
struct ProviderErrorBody {
    error: Option<ProviderErrorDetail>,
}

#[derive(Deserialize)]
#[cfg(feature = "native-client")]
struct ProviderErrorDetail {
    message: String,
}
