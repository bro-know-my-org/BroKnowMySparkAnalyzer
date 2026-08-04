mod agent;
mod client;
mod config;
mod error;
mod evidence;
mod prompt;
mod types;

pub use agent::{
    ask_follow_up, required_tools_for_kind, run_analysis, run_tool_agent, ToolExecutor,
};
pub use bkmsa_core::ReportKind;
pub use client::ChatClient;
#[cfg(feature = "native-client")]
pub use client::OpenAiClient;
pub use config::AiConfig;
pub use error::{AgentError, Result};
pub use types::{
    AgentOptions, AgentResult, AgentTrace, ChatMessage, ChatRole, FollowUpMessage, FollowUpRole,
    ModelInfo, ReportContext, TraceRole,
};
