pub mod cli;
pub mod config;
pub mod error;
pub mod input;
pub mod output;

use clap::Parser;
use cli::{Cli, Command, ToolArgs};
use error::CliError;
use serde_json::{json, Map, Value};

pub async fn run_from_env() -> Result<(), CliError> {
    run(Cli::parse()).await
}

pub async fn run(cli: Cli) -> Result<(), CliError> {
    match cli.command {
        Command::Tools(args) => {
            let tools = bkmsa_core::report_tool_descriptions();
            let text = output::render_json(args.format, "BKMSA report tools", &tools)?;
            output::emit(&text, args.output.as_deref()).await
        }
        Command::Inventory(args) => {
            let report = parse_source(&args.source, args.text).await?;
            let value = bkmsa_core::execute_tool(&report, "report_inventory", json!({}))
                .map_err(|error| CliError::Analysis(error.to_string()))?;
            let value = envelope("inventory", &report, value);
            let text = output::render_value(args.output.format, "Report inventory", &value)?;
            output::emit(&text, args.output.output.as_deref()).await
        }
        Command::Inspect(args) => {
            let report = parse_source(&args.source, args.text).await?;
            let value = json!({
                "command": "inspect",
                "source": report.source,
                "kind": report.kind.as_str(),
                "summary": report.summary,
            });
            let text = output::render_value(args.output.format, "Report summary", &value)?;
            output::emit(&text, args.output.output.as_deref()).await
        }
        Command::Tool(args) => run_tool(args).await,
        Command::Analyze(args) => run_analyze(args).await,
    }
}

async fn parse_source(source: &str, force_text: bool) -> Result<bkmsa_core::Report, CliError> {
    let input = input::load_report(source).await?;
    let text_hint = matches!(
        input.hint.to_ascii_lowercase().as_str(),
        "txt" | "log" | "md"
    );
    if force_text {
        let text = String::from_utf8(input.bytes).map_err(|error| {
            CliError::Decode(format!("text report is not valid UTF-8: {error}"))
        })?;
        return bkmsa_core::parse_text_report(text, input.source)
            .map_err(|error| CliError::Decode(error.to_string()));
    }

    if text_hint && looks_like_text(&input.bytes) {
        let text = String::from_utf8(input.bytes).map_err(|error| {
            CliError::Decode(format!("text report is not valid UTF-8: {error}"))
        })?;
        return bkmsa_core::parse_text_report(text, input.source)
            .map_err(|error| CliError::Decode(error.to_string()));
    }

    bkmsa_core::parse_report_bytes(&input.bytes, input.source, &input.hint)
        .map_err(|error| CliError::Decode(error.to_string()))
}

fn looks_like_text(bytes: &[u8]) -> bool {
    if std::str::from_utf8(bytes).is_err() || bytes.contains(&0) {
        return false;
    }
    let suspicious = bytes
        .iter()
        .filter(|byte| byte.is_ascii_control() && !matches!(byte, b'\n' | b'\r' | b'\t' | 0x1b))
        .count();
    suspicious <= 4.max(bytes.len() / 100)
}

async fn run_tool(args: ToolArgs) -> Result<(), CliError> {
    let report = parse_source(&args.source, args.text).await?;
    let mut pairs = args.arg;
    if let Some(category) = args.category {
        pairs.push(format!("category={category}"));
    }
    if let Some(limit) = args.limit {
        pairs.push(format!("limit={limit}"));
    }
    if let Some(path) = args.path {
        pairs.push(format!("path={path}"));
    }
    let tool_args = parse_tool_args(args.args.as_deref(), &pairs)?;
    let tool_name = args.tool.replace('-', "_");
    let result = bkmsa_core::execute_tool(&report, &tool_name, tool_args)
        .map_err(|error| CliError::Analysis(error.to_string()))?;
    let value = envelope(&tool_name, &report, result);
    let text = output::render_value(args.output.format, &format!("Tool: {tool_name}"), &value)?;
    output::emit(&text, args.output.output.as_deref()).await
}

async fn run_analyze(args: cli::AnalyzeArgs) -> Result<(), CliError> {
    let report = parse_source(&args.source, args.text).await?;
    let file_config = config::load(args.config.as_deref()).await?;
    let api_key = args
        .api_key
        .filter(|value| !value.trim().is_empty())
        .or_else(|| file_config.api_key.filter(|value| !value.trim().is_empty()))
        .ok_or_else(|| CliError::Config("BKMSA_API_KEY is required for analyze".into()))?;
    let config = bkmsa_agent::AiConfig::new(
        args.base_url
            .filter(|value| !value.trim().is_empty())
            .or_else(|| {
                file_config
                    .base_url
                    .filter(|value| !value.trim().is_empty())
            })
            .unwrap_or_else(|| "https://api.openai.com/v1".into()),
        api_key,
        args.model
            .filter(|value| !value.trim().is_empty())
            .or_else(|| file_config.model.filter(|value| !value.trim().is_empty()))
            .unwrap_or_else(|| "gpt-4.1-mini".into()),
        args.temperature.or(file_config.temperature).unwrap_or(0.2),
    )
    .map_err(|error| CliError::Config(error.to_string()))?;
    let client = bkmsa_agent::OpenAiClient::new(config)
        .map_err(|error| CliError::Config(error.to_string()))?;
    let options = bkmsa_agent::AgentOptions {
        max_rounds: args.max_rounds,
        ..Default::default()
    };
    let result = bkmsa_agent::run_tool_agent(&report, &client, options, |_| {})
        .await
        .map_err(map_agent_error)?;
    let value =
        serde_json::to_value(&result).map_err(|error| CliError::Analysis(error.to_string()))?;
    let text = output::render_diagnosis(args.output.format, &result.diagnosis, &value)?;
    output::emit(&text, args.output.output.as_deref()).await
}

fn map_agent_error(error: bkmsa_agent::AgentError) -> CliError {
    match error {
        bkmsa_agent::AgentError::MissingConfig(_) | bkmsa_agent::AgentError::InvalidConfig(_) => {
            CliError::Config(error.to_string())
        }
        bkmsa_agent::AgentError::Http(_)
        | bkmsa_agent::AgentError::Provider { .. }
        | bkmsa_agent::AgentError::EmptyResponse
        | bkmsa_agent::AgentError::Refusal(_) => CliError::Provider(error.to_string()),
        bkmsa_agent::AgentError::Tool { .. } | bkmsa_agent::AgentError::Json(_) => {
            CliError::Analysis(error.to_string())
        }
    }
}

fn envelope(command: &str, report: &bkmsa_core::Report, result: Value) -> Value {
    json!({
        "command": command,
        "source": report.source,
        "kind": report.kind.as_str(),
        "result": result,
    })
}

pub fn parse_tool_args(json_args: Option<&str>, pairs: &[String]) -> Result<Value, CliError> {
    if let Some(raw) = json_args {
        let value: Value = serde_json::from_str(raw)
            .map_err(|error| CliError::Config(format!("invalid --args JSON: {error}")))?;
        if !value.is_object() {
            return Err(CliError::Config("--args must be a JSON object".into()));
        }
        return Ok(value);
    }

    let mut args = Map::new();
    for pair in pairs {
        let (key, raw) = pair.split_once('=').ok_or_else(|| {
            CliError::Config(format!("invalid --arg '{pair}'; expected KEY=VALUE"))
        })?;
        if key.trim().is_empty() {
            return Err(CliError::Config("tool argument key cannot be empty".into()));
        }
        let value = serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_owned()));
        let key = key.trim().to_owned();
        if args.insert(key.clone(), value).is_some() {
            return Err(CliError::Config(format!(
                "duplicate tool argument key: {key}"
            )));
        }
    }
    Ok(Value::Object(args))
}

#[cfg(test)]
mod tests {
    use super::looks_like_text;

    #[test]
    fn text_detection_accepts_ansi_logs_and_rejects_binary_nul() {
        assert!(looks_like_text(b"\x1b[31mCan't keep up!\x1b[0m\n"));
        assert!(!looks_like_text(b"spark\0protobuf"));
    }
}
