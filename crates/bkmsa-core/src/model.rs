use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReportKind {
    Sampler,
    Health,
    Heap,
    Text,
}

impl ReportKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sampler => "sampler",
            Self::Health => "health",
            Self::Heap => "heap",
            Self::Text => "text",
        }
    }
}

impl fmt::Display for ReportKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Critical,
    Warning,
    Info,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub severity: Severity,
    pub title: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NamedValue {
    pub name: String,
    pub value: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HeapHotspot {
    #[serde(rename = "type")]
    pub type_name: String,
    pub instances: u64,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StackHotspot {
    pub label: String,
    pub samples: f64,
    pub percent: f64,
    pub thread: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method_desc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_number: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReportSummary {
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tps1m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tps5m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tps15m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mspt_median: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mspt_p95: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mspt_max: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process_cpu1m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_cpu1m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heap_used_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heap_max_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entity_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub player_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gc: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub worlds: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub top_entities: Vec<NamedValue>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub top_heap: Vec<HeapHotspot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub top_hotspots: Vec<StackHotspot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<Finding>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Report {
    pub kind: ReportKind,
    pub source: String,
    pub raw: Value,
    pub summary: ReportSummary,
}

pub type ToolResult = Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolRequest {
    pub tool: String,
    #[serde(default = "empty_args")]
    pub args: Value,
}

fn empty_args() -> Value {
    serde_json::json!({})
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ToolDescription {
    pub name: &'static str,
    pub args: Value,
    pub description: &'static str,
}

#[derive(Debug, thiserror::Error)]
pub enum SparkError {
    #[error("无法按 spark protobuf 解码: {0}")]
    Decode(String),
    #[error("未知报告工具: {0}")]
    UnknownTool(String),
    #[error("工具参数错误: {0}")]
    InvalidArgument(String),
    #[error("报告数据错误: {0}")]
    InvalidReport(String),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
