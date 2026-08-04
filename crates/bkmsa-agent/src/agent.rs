use std::collections::BTreeSet;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    evidence::{self, EvidenceState},
    prompt, AgentError, AgentOptions, AgentResult, AgentTrace, ChatClient, ChatMessage,
    FollowUpMessage, FollowUpRole, ReportContext, ReportKind, Result, TraceRole,
};

#[cfg(not(target_arch = "wasm32"))]
#[async_trait]
pub trait ToolExecutor: Send + Sync {
    fn context(&self) -> ReportContext;

    async fn execute_tool(&self, tool: &str, args: Value) -> std::result::Result<Value, String>;
}

#[cfg(target_arch = "wasm32")]
#[async_trait(?Send)]
pub trait ToolExecutor {
    fn context(&self) -> ReportContext;

    async fn execute_tool(&self, tool: &str, args: Value) -> std::result::Result<Value, String>;
}

#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl ToolExecutor for bkmsa_core::Report {
    fn context(&self) -> ReportContext {
        ReportContext {
            kind: self.kind,
            source: self.source.clone(),
            summary: serde_json::to_value(&self.summary).unwrap_or(Value::Null),
        }
    }

    async fn execute_tool(&self, tool: &str, args: Value) -> std::result::Result<Value, String> {
        bkmsa_core::execute_tool(self, tool, args).map_err(|error| error.to_string())
    }
}

pub fn required_tools_for_kind(kind: ReportKind) -> &'static [&'static str] {
    prompt::required_tools(kind)
}

pub async fn run_analysis<E, C>(
    report: &E,
    client: &C,
    options: AgentOptions,
) -> Result<AgentResult>
where
    E: ToolExecutor,
    C: ChatClient,
{
    run_tool_agent(report, client, options, |_| {}).await
}

pub async fn run_tool_agent<E, C, F>(
    report: &E,
    client: &C,
    options: AgentOptions,
    mut on_trace: F,
) -> Result<AgentResult>
where
    E: ToolExecutor,
    C: ChatClient,
    F: FnMut(&AgentTrace),
{
    validate_options(&options)?;
    let context = report.context();
    let required_tools = required_tools_for_kind(context.kind);
    let mut used_tools = BTreeSet::from(["report_inventory".to_owned()]);
    let mut evidence_state = EvidenceState::default();
    let mut traces = Vec::new();

    let inventory = execute(report, "report_inventory", json!({})).await?;
    let inventory_text = pretty_json(&inventory)?;
    emit(
        &mut traces,
        &mut on_trace,
        AgentTrace {
            round: 0,
            role: TraceRole::Tool,
            title: "Tool: report_inventory".into(),
            content: inventory_text.clone(),
        },
    );

    let mut messages = vec![
        ChatMessage::system(prompt::system_prompt(required_tools)),
        ChatMessage::user(prompt::initial_user_prompt(&inventory_text, required_tools)),
    ];

    for round in 1..=options.max_rounds {
        let content = client.chat(&messages).await?;
        if content.chars().count() > 256 * 1024 {
            return Err(AgentError::Provider {
                status: 0,
                message: "provider response exceeds the 256K character agent limit".into(),
            });
        }
        emit(
            &mut traces,
            &mut on_trace,
            AgentTrace {
                round,
                role: TraceRole::Assistant,
                title: "AI".into(),
                content: content.clone(),
            },
        );

        if let Some(call) = parse_tool_call(&content) {
            if !known_tool(&call.tool) {
                append_tool_error(
                    &mut messages,
                    &mut traces,
                    &mut on_trace,
                    round,
                    content,
                    &call.tool,
                    "tool is not in the advertised read-only registry",
                );
                continue;
            }
            let result = match execute(report, &call.tool, call.args).await {
                Ok(result) => result,
                Err(error) => {
                    append_tool_error(
                        &mut messages,
                        &mut traces,
                        &mut on_trace,
                        round,
                        content,
                        &call.tool,
                        &error.to_string(),
                    );
                    continue;
                }
            };
            used_tools.insert(call.tool.clone());
            evidence::update(&mut evidence_state, &call.tool, &result);
            append_tool_result(
                &mut messages,
                &mut traces,
                &mut on_trace,
                round,
                content,
                &call.tool,
                &result,
                &used_tools,
                required_tools,
                options.max_tool_result_chars,
            )?;
            continue;
        }

        let missing = missing_tools(required_tools, &used_tools);
        if let Some(tool) = missing.first().copied() {
            let result = execute(report, tool, default_args(tool)).await?;
            used_tools.insert(tool.to_owned());
            evidence::update(&mut evidence_state, tool, &result);
            emit(
                &mut traces,
                &mut on_trace,
                AgentTrace {
                    round,
                    role: TraceRole::System,
                    title: "Premature final blocked".into(),
                    content: format!("AI 在查完必要工具前尝试收口。强制补查 {tool}。"),
                },
            );
            let result_text = bounded_json(&result, options.max_tool_result_chars)?;
            emit(
                &mut traces,
                &mut on_trace,
                AgentTrace {
                    round,
                    role: TraceRole::Tool,
                    title: format!("Tool: {tool}"),
                    content: result_text.clone(),
                },
            );
            messages.push(ChatMessage::assistant(content));
            messages.push(ChatMessage::user(format!(
                "你刚才过早输出最终诊断。系统已强制补查 TOOL_RESULT {tool}:\n{}\n\
还没查完的必要工具：{}。继续；未查完时只输出 JSON 工具调用。",
                result_text,
                missing_tools(required_tools, &used_tools).join(", ")
            )));
            continue;
        }

        if round <= options.validation_round_limit {
            if let Some(problem) = evidence::validate_final(&content, &evidence_state) {
                let correction = problem.correction(&evidence_state);
                emit(
                    &mut traces,
                    &mut on_trace,
                    AgentTrace {
                        round,
                        role: TraceRole::System,
                        title: "Evidence validation blocked".into(),
                        content: correction.clone(),
                    },
                );
                messages.push(ChatMessage::assistant(content));
                messages.push(ChatMessage::user(format!(
                    "{correction}\n重新输出最终 Markdown，并保持 # 结论、# 证据链、# 排除项、# 还不能确定的点、# 立刻执行。"
                )));
                continue;
            }
        }

        return Ok(AgentResult {
            diagnosis: content,
            traces,
            used_tools: used_tools.into_iter().collect(),
            rounds: round,
            reached_round_limit: false,
        });
    }

    Ok(AgentResult {
        diagnosis: "达到最大工具轮数。请缩小问题或增加 max rounds。".into(),
        traces,
        used_tools: used_tools.into_iter().collect(),
        rounds: options.max_rounds,
        reached_round_limit: true,
    })
}

pub async fn ask_follow_up<C: ChatClient>(
    report: &ReportContext,
    client: &C,
    traces: &[AgentTrace],
    diagnosis: &str,
    history: &[FollowUpMessage],
    question: &str,
) -> Result<String> {
    if question.len() > 32 * 1024 || history.len() > 64 {
        return Err(AgentError::InvalidConfig(
            "follow-up question or history exceeds the request limit".into(),
        ));
    }
    let tool_context = traces
        .iter()
        .filter(|trace| matches!(trace.role, TraceRole::Tool | TraceRole::System))
        .rev()
        .take(12)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|trace| format!("{}\n{}", trace.title, truncate_chars(&trace.content, 5_000)))
        .collect::<Vec<_>>()
        .join("\n\n---\n\n");
    let report_summary = json!({
        "kind": report.kind.as_str(),
        "source": report.source,
        "summary": report.summary,
    });
    let mut messages = vec![
        ChatMessage::system(prompt::follow_up_system_prompt()),
        ChatMessage::user(format!(
            "当前报告摘要:\n{}\n\n当前诊断结论:\n{}\n\n已调用工具证据:\n{}",
            truncate_chars(&pretty_json(&report_summary)?, 10_000),
            truncate_chars(diagnosis, 12_000),
            truncate_chars(&tool_context, 32_000)
        )),
    ];
    messages.extend(
        history
            .iter()
            .rev()
            .take(8)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|item| ChatMessage {
                role: match item.role {
                    FollowUpRole::User => crate::ChatRole::User,
                    FollowUpRole::Assistant => crate::ChatRole::Assistant,
                },
                content: truncate_chars(&item.content, 16_000),
            }),
    );
    messages.push(ChatMessage::user(question));
    client.chat(&messages).await
}

fn validate_options(options: &AgentOptions) -> Result<()> {
    if !(1..=64).contains(&options.max_rounds) {
        return Err(AgentError::InvalidConfig(
            "max_rounds must be between 1 and 64".into(),
        ));
    }
    if options.validation_round_limit > options.max_rounds {
        return Err(AgentError::InvalidConfig(
            "validation_round_limit cannot exceed max_rounds".into(),
        ));
    }
    if !(1_024..=64 * 1024).contains(&options.max_tool_result_chars) {
        return Err(AgentError::InvalidConfig(
            "max_tool_result_chars must be between 1024 and 65536".into(),
        ));
    }
    Ok(())
}

async fn execute<E: ToolExecutor>(report: &E, tool: &str, args: Value) -> Result<Value> {
    report
        .execute_tool(tool, args)
        .await
        .map_err(|message| AgentError::Tool {
            tool: tool.into(),
            message,
        })
}

#[allow(clippy::too_many_arguments)]
fn append_tool_result<F: FnMut(&AgentTrace)>(
    messages: &mut Vec<ChatMessage>,
    traces: &mut Vec<AgentTrace>,
    on_trace: &mut F,
    round: usize,
    assistant_content: String,
    tool: &str,
    result: &Value,
    used_tools: &BTreeSet<String>,
    required_tools: &[&str],
    max_chars: usize,
) -> Result<()> {
    let result_text = bounded_json(result, max_chars)?;
    emit(
        traces,
        on_trace,
        AgentTrace {
            round,
            role: TraceRole::Tool,
            title: format!("Tool: {tool}"),
            content: result_text.clone(),
        },
    );
    messages.push(ChatMessage::assistant(assistant_content));
    messages.push(ChatMessage::user(format!(
        "TOOL_RESULT {tool}:\n{}\n已查工具：{}\n必要但未查工具：{}\n\
继续。必要工具未查完时只允许输出 JSON 工具调用；查完后如证据足够再输出最终 Markdown。",
        result_text,
        used_tools.iter().cloned().collect::<Vec<_>>().join(", "),
        missing_tools(required_tools, used_tools).join(", ")
    )));
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn append_tool_error<F: FnMut(&AgentTrace)>(
    messages: &mut Vec<ChatMessage>,
    traces: &mut Vec<AgentTrace>,
    on_trace: &mut F,
    round: usize,
    assistant_content: String,
    tool: &str,
    message: &str,
) {
    let message = truncate_with_marker(message, 2_000);
    emit(
        traces,
        on_trace,
        AgentTrace {
            round,
            role: TraceRole::System,
            title: format!("Tool error: {tool}"),
            content: message.clone(),
        },
    );
    messages.push(ChatMessage::assistant(assistant_content));
    messages.push(ChatMessage::user(format!(
        "TOOL_ERROR {tool}: {message}\n请选择已公布的只读工具并使用有效参数重试。"
    )));
}

fn emit<F: FnMut(&AgentTrace)>(
    traces: &mut Vec<AgentTrace>,
    on_trace: &mut F,
    mut trace: AgentTrace,
) {
    const MAX_TRACE_ITEM_CHARS: usize = 128 * 1024;
    const MAX_TRACE_TOTAL_CHARS: usize = 2 * 1024 * 1024;
    let used = traces
        .iter()
        .map(|item| item.title.chars().count() + item.content.chars().count())
        .sum::<usize>();
    let remaining = MAX_TRACE_TOTAL_CHARS.saturating_sub(used);
    if remaining == 0 {
        return;
    }
    trace.title = truncate_with_marker(&trace.title, remaining.min(1_024));
    let content_limit = remaining
        .saturating_sub(trace.title.chars().count())
        .min(MAX_TRACE_ITEM_CHARS);
    trace.content = if content_limit == 0 {
        String::new()
    } else {
        truncate_with_marker(&trace.content, content_limit)
    };
    on_trace(&trace);
    traces.push(trace);
}

fn missing_tools<'a>(required: &'a [&'a str], used: &BTreeSet<String>) -> Vec<&'a str> {
    required
        .iter()
        .copied()
        .filter(|tool| !used.contains(*tool))
        .collect()
}

fn default_args(tool: &str) -> Value {
    match tool {
        "hotspots" => json!({"limit": 32}),
        "hotspot_groups" => json!({"limit": 24}),
        "hot_paths" => json!({"category": "auto", "limit": 64}),
        "mod_sources" => json!({"limit": 24}),
        "time_windows" => json!({"limit": 80}),
        "worst_windows" => json!({"limit": 16}),
        "entity_chunks" => json!({"limit": 24}),
        "evidence_links" => json!({"limit": 16}),
        "heap" => json!({"limit": 40}),
        _ => json!({}),
    }
}

fn known_tool(tool: &str) -> bool {
    tool == "report_inventory"
        || bkmsa_core::report_tool_descriptions()
            .iter()
            .any(|description| description.name == tool)
}

#[derive(Deserialize)]
struct ToolCall {
    tool: String,
    #[serde(default = "empty_object")]
    args: Value,
}

fn empty_object() -> Value {
    json!({})
}

fn parse_tool_call(content: &str) -> Option<ToolCall> {
    let trimmed = content.trim();
    let mut candidates = vec![trimmed];
    if let Some(fence_start) = trimmed.find("```") {
        let after = &trimmed[fence_start + 3..];
        let after = after.strip_prefix("json").unwrap_or(after).trim_start();
        if let Some(fence_end) = after.find("```") {
            candidates.push(&after[..fence_end]);
        }
    }
    candidates.into_iter().find_map(|candidate| {
        let parsed: ToolCall = serde_json::from_str(candidate).ok()?;
        (!parsed.tool.trim().is_empty() && parsed.args.is_object()).then_some(parsed)
    })
}

fn pretty_json(value: &Value) -> Result<String> {
    Ok(serde_json::to_string_pretty(value)?)
}
fn bounded_json(value: &Value, limit: usize) -> Result<String> {
    let full = pretty_json(value)?;
    if full.chars().count() <= limit {
        return Ok(full);
    }
    let preview_limit = limit.saturating_sub(256).max(32);
    Ok(serde_json::to_string_pretty(&json!({
        "truncated": true,
        "originalChars": full.chars().count(),
        "preview": truncate_with_marker(&full, preview_limit),
    }))?)
}
fn truncate_chars(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}
fn truncate_with_marker(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        value.to_owned()
    } else {
        format!("{}… [truncated]", truncate_chars(value, limit))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct FakeReport {
        context: ReportContext,
        calls: Mutex<Vec<String>>,
    }

    #[cfg_attr(not(target_arch = "wasm32"), async_trait)]
    #[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
    impl ToolExecutor for FakeReport {
        fn context(&self) -> ReportContext {
            self.context.clone()
        }
        async fn execute_tool(
            &self,
            tool: &str,
            _args: Value,
        ) -> std::result::Result<Value, String> {
            self.calls.lock().unwrap().push(tool.into());
            Ok(json!({"tool": tool}))
        }
    }

    struct FakeClient {
        responses: Mutex<Vec<String>>,
    }

    #[cfg_attr(not(target_arch = "wasm32"), async_trait)]
    #[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
    impl ChatClient for FakeClient {
        async fn chat(&self, _messages: &[ChatMessage]) -> Result<String> {
            Ok(self.responses.lock().unwrap().remove(0))
        }
    }

    fn text_report() -> FakeReport {
        FakeReport {
            context: ReportContext {
                kind: ReportKind::Text,
                source: "fixture".into(),
                summary: json!({}),
            },
            calls: Mutex::new(Vec::new()),
        }
    }

    #[test]
    fn parses_plain_and_fenced_tool_calls() {
        assert_eq!(
            parse_tool_call(r#"{"tool":"overview","args":{}}"#)
                .unwrap()
                .tool,
            "overview"
        );
        assert_eq!(
            parse_tool_call("```json\n{\"tool\":\"heap\",\"args\":{}}\n```")
                .unwrap()
                .tool,
            "heap"
        );
        assert!(
            parse_tool_call("最终答案示例：{\"tool\":\"overview\",\"args\":{}}，不应执行。")
                .is_none()
        );
    }

    #[test]
    fn bounded_tool_results_remain_valid_json() {
        let value = json!({"large":"x".repeat(10_000)});
        let text = bounded_json(&value, 500).unwrap();
        let parsed: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["truncated"], true);
    }

    #[tokio::test]
    async fn forces_required_tools_before_accepting_final() {
        let report = text_report();
        let client = FakeClient { responses: Mutex::new(vec![
            "# 结论\n过早".into(),
            "# 结论\n仍然过早".into(),
            "# 结论\n确定结论\n# 证据链\n证据\n# 排除项\n无\n# 还不能确定的点\n无\n# 立刻执行\n复测".into(),
        ]) };
        let result = run_analysis(&report, &client, AgentOptions::default())
            .await
            .unwrap();
        let calls = report.calls.lock().unwrap().clone();
        assert_eq!(calls, vec!["report_inventory", "overview", "evidence_gaps"]);
        assert!(!result.reached_round_limit);
        assert!(result
            .traces
            .iter()
            .any(|trace| trace.title == "Premature final blocked"));
    }

    #[tokio::test]
    async fn tool_call_round_trip_is_recorded() {
        let report = text_report();
        let client = FakeClient { responses: Mutex::new(vec![
            r#"{"tool":"overview","args":{}}"#.into(),
            r#"{"tool":"evidence_gaps","args":{}}"#.into(),
            "# 结论\n确定结论\n# 证据链\n证据\n# 排除项\n无\n# 还不能确定的点\n无\n# 立刻执行\n复测".into(),
        ]) };
        let result = run_analysis(&report, &client, AgentOptions::default())
            .await
            .unwrap();
        assert_eq!(
            result.used_tools,
            vec!["evidence_gaps", "overview", "report_inventory"]
        );
    }
}
