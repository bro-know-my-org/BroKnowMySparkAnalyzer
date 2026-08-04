use bkmsa_core::ReportKind;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReportContext {
    pub kind: ReportKind,
    pub source: String,
    pub summary: Value,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TraceRole {
    Assistant,
    Tool,
    System,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AgentTrace {
    pub round: usize,
    pub role: TraceRole,
    pub title: String,
    pub content: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    System,
    User,
    Assistant,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FollowUpRole {
    User,
    Assistant,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::System,
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::User,
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: ChatRole::Assistant,
            content: content.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FollowUpMessage {
    pub role: FollowUpRole,
    pub content: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
}

#[derive(Clone, Debug)]
pub struct AgentOptions {
    pub max_rounds: usize,
    pub validation_round_limit: usize,
    pub max_tool_result_chars: usize,
}

impl Default for AgentOptions {
    fn default() -> Self {
        Self {
            max_rounds: 12,
            validation_round_limit: 10,
            max_tool_result_chars: 18_000,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentResult {
    pub diagnosis: String,
    pub traces: Vec<AgentTrace>,
    pub used_tools: Vec<String>,
    pub rounds: usize,
    pub reached_round_limit: bool,
}
