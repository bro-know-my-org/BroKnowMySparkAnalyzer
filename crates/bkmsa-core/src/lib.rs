mod analysis;
mod environment;
mod hot_paths;
mod memory_gc;
mod model;
mod parser;
mod tools;
mod windows;

pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/spark.rs"));
}

pub use model::{
    Finding, HeapHotspot, NamedValue, Report, ReportKind, ReportSummary, Severity, SparkError,
    StackHotspot, ToolDescription, ToolRequest, ToolResult,
};
pub use parser::{parse_report_bytes, parse_text_report};
pub use tools::{execute_tool, execute_tool_request, report_tool_descriptions};
