#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("missing required configuration: {0}")]
    MissingConfig(&'static str),
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
    #[cfg(feature = "native-client")]
    #[error("AI provider request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("AI provider returned HTTP {status}: {message}")]
    Provider { status: u16, message: String },
    #[error("AI provider returned no assistant content")]
    EmptyResponse,
    #[error("AI provider refused the request: {0}")]
    Refusal(String),
    #[error("report tool `{tool}` failed: {message}")]
    Tool { tool: String, message: String },
    #[error("failed to process agent JSON: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, AgentError>;
