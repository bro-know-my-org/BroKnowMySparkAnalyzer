use crate::{cli::OutputFormat, error::CliError};
use serde::Serialize;
use serde_json::Value;
use std::path::Path;
use tokio::io::AsyncWriteExt;

pub fn render_value(format: OutputFormat, title: &str, value: &Value) -> Result<String, CliError> {
    match format {
        OutputFormat::Json => serde_json::to_string_pretty(value)
            .map(|text| format!("{text}\n"))
            .map_err(|error| CliError::Output(error.to_string())),
        OutputFormat::Markdown => Ok(format!(
            "# {}\n\n```json\n{}\n```\n",
            markdown_heading(title),
            serde_json::to_string_pretty(value)
                .map_err(|error| CliError::Output(error.to_string()))?
        )),
        OutputFormat::Terminal => Ok(render_terminal(value, 0)),
    }
}

pub fn render_json<T: Serialize>(
    format: OutputFormat,
    title: &str,
    value: &T,
) -> Result<String, CliError> {
    let value = serde_json::to_value(value).map_err(|error| CliError::Output(error.to_string()))?;
    render_value(format, title, &value)
}

pub fn render_diagnosis(
    format: OutputFormat,
    diagnosis: &str,
    json: &Value,
) -> Result<String, CliError> {
    match format {
        OutputFormat::Terminal => Ok(format!("{}\n", escape_terminal(diagnosis.trim_end()))),
        OutputFormat::Markdown => Ok(format!("{}\n", diagnosis.trim_end())),
        OutputFormat::Json => render_value(format, "Analysis", json),
    }
}

pub async fn emit(text: &str, output: Option<&Path>) -> Result<(), CliError> {
    if let Some(path) = output {
        tokio::fs::write(path, text)
            .await
            .map_err(|error| CliError::Output(format!("{}: {error}", path.display())))
    } else {
        tokio::io::stdout()
            .write_all(text.as_bytes())
            .await
            .map_err(|error| CliError::Output(format!("stdout: {error}")))
    }
}

fn render_terminal(value: &Value, depth: usize) -> String {
    if depth >= 64 {
        return format!("{}<depth limit reached>\n", "  ".repeat(depth));
    }
    match value {
        Value::Object(map) => {
            if map.is_empty() {
                return format!("{}{{}}\n", "  ".repeat(depth));
            }
            let mut output = String::new();
            for (key, value) in map {
                let indent = "  ".repeat(depth);
                match value {
                    Value::Array(_) | Value::Object(_) => {
                        output.push_str(&format!("{indent}{}:\n", escape_terminal(key)));
                        output.push_str(&render_terminal(value, depth + 1));
                    }
                    _ => output.push_str(&format!(
                        "{indent}{}: {}\n",
                        escape_terminal(key),
                        scalar(value)
                    )),
                }
            }
            output
        }
        Value::Array(values) => {
            if values.is_empty() {
                return format!("{}[]\n", "  ".repeat(depth));
            }
            values
                .iter()
                .map(|value| {
                    let indent = "  ".repeat(depth);
                    match value {
                        Value::Object(_) | Value::Array(_) => {
                            format!("{indent}-\n{}", render_terminal(value, depth + 1))
                        }
                        _ => format!("{indent}- {}\n", scalar(value)),
                    }
                })
                .collect()
        }
        _ => format!("{}{}\n", "  ".repeat(depth), scalar(value)),
    }
}

fn scalar(value: &Value) -> String {
    match value {
        Value::String(value) => escape_terminal(value),
        Value::Null => "null".into(),
        other => other.to_string(),
    }
}

fn escape_terminal(value: &str) -> String {
    value
        .chars()
        .flat_map(|character| {
            if character.is_control() {
                character.escape_default().collect::<Vec<_>>()
            } else {
                vec![character]
            }
        })
        .collect()
}

fn markdown_heading(value: &str) -> String {
    value
        .chars()
        .flat_map(|character| {
            if character.is_control() {
                vec![' ']
            } else if matches!(character, '#' | '[' | ']' | '`' | '\\') {
                vec!['\\', character]
            } else {
                vec![character]
            }
        })
        .collect()
}
