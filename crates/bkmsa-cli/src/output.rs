use crate::{cli::OutputFormat, error::CliError};
use serde::Serialize;
use serde_json::Value;
use std::fmt::Write as _;
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
        OutputFormat::Terminal => {
            let mut output = String::new();
            render_terminal(value, 0, &mut output);
            Ok(output)
        }
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
        let mut stdout = tokio::io::stdout();
        stdout
            .write_all(text.as_bytes())
            .await
            .map_err(|error| CliError::Output(format!("stdout: {error}")))?;
        stdout
            .flush()
            .await
            .map_err(|error| CliError::Output(format!("stdout flush: {error}")))
    }
}

fn render_terminal(value: &Value, depth: usize, output: &mut String) {
    if depth >= 64 {
        let _ = writeln!(output, "{}<depth limit reached>", "  ".repeat(depth));
        return;
    }
    match value {
        Value::Object(map) => {
            if map.is_empty() {
                let _ = writeln!(output, "{}{{}}", "  ".repeat(depth));
                return;
            }
            for (key, value) in map {
                let indent = "  ".repeat(depth);
                match value {
                    Value::Array(_) | Value::Object(_) => {
                        let _ = writeln!(output, "{indent}{}:", escape_terminal(key));
                        render_terminal(value, depth + 1, output);
                    }
                    _ => {
                        let _ = writeln!(
                            output,
                            "{indent}{}: {}",
                            escape_terminal(key),
                            scalar(value)
                        );
                    }
                }
            }
        }
        Value::Array(values) => {
            if values.is_empty() {
                let _ = writeln!(output, "{}[]", "  ".repeat(depth));
                return;
            }
            for value in values {
                let indent = "  ".repeat(depth);
                match value {
                    Value::Object(_) | Value::Array(_) => {
                        let _ = writeln!(output, "{indent}-");
                        render_terminal(value, depth + 1, output);
                    }
                    _ => {
                        let _ = writeln!(output, "{indent}- {}", scalar(value));
                    }
                }
            }
        }
        _ => {
            let _ = writeln!(output, "{}{}", "  ".repeat(depth), scalar(value));
        }
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
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_control()
            || matches!(character, '\u{2028}' | '\u{2029}')
            || is_bidi_control(character)
        {
            for escaped in character.escape_default() {
                output.push(escaped);
            }
        } else {
            output.push(character);
        }
    }
    output
}

fn is_bidi_control(character: char) -> bool {
    matches!(
        character,
        '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn terminal_rendering_escapes_controls_and_bidi_characters() {
        let value = json!({"key\u{202e}":"line\n\u{061c}\u{2069}"});
        let rendered = render_value(OutputFormat::Terminal, "ignored", &value).unwrap();
        assert!(rendered.contains("\\u{202e}"));
        assert!(rendered.contains("\\n"));
        assert!(rendered.contains("\\u{61c}"));
        assert!(rendered.contains("\\u{2069}"));
        assert!(!rendered.contains('\u{202e}'));
    }

    #[test]
    fn terminal_rendering_handles_empty_collections_and_depth_boundary() {
        assert_eq!(
            render_value(OutputFormat::Terminal, "ignored", &json!({"a":[],"b":{}})).unwrap(),
            "a:\n  []\nb:\n  {}\n"
        );
        let mut value = json!("leaf");
        for _ in 0..65 {
            value = json!([value]);
        }
        let rendered = render_value(OutputFormat::Terminal, "ignored", &value).unwrap();
        assert!(rendered.contains("<depth limit reached>"));
    }

    #[test]
    fn bidi_boundaries_do_not_escape_adjacent_characters() {
        assert_eq!(
            escape_terminal("a\u{202a}b\u{202e}c"),
            "a\\u{202a}b\\u{202e}c"
        );
        assert_eq!(
            escape_terminal("\u{2028}\u{2029}\u{202f}"),
            "\\u{2028}\\u{2029}\u{202f}"
        );
        assert_eq!(
            escape_terminal("\u{2065}\u{2066}\u{2069}\u{206a}"),
            "\u{2065}\\u{2066}\\u{2069}\u{206a}"
        );
        for control in [
            '\u{061c}', '\u{200e}', '\u{200f}', '\u{202a}', '\u{202e}', '\u{2066}', '\u{2069}',
        ] {
            assert!(escape_terminal(&control.to_string()).starts_with("\\u{"));
        }
        for ordinary in [
            '\u{061b}', '\u{061d}', '\u{200d}', '\u{2010}', '\u{202f}', '\u{2065}', '\u{206a}',
        ] {
            assert_eq!(escape_terminal(&ordinary.to_string()), ordinary.to_string());
        }
    }
}
