use base64::Engine;
use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use url::Url;

#[derive(Debug, Serialize)]
struct RemoteReport {
    bytes_base64: String,
    content_type: String,
    resolved_url: String,
}

#[derive(Debug, Deserialize)]
struct AiConfig {
    base_url: String,
    api_key: String,
    model: String,
    temperature: f32,
}

#[derive(Debug, Deserialize)]
struct AiRequest {
    config: AiConfig,
    prompt: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
struct AiMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct AiChatRequest {
    config: AiConfig,
    messages: Vec<AiMessage>,
}

#[derive(Debug, Deserialize)]
struct SaveExportRequest {
    path: String,
    bytes_base64: String,
}

#[derive(Debug, Serialize)]
struct AiModelInfo {
    id: String,
}

#[tauri::command]
async fn fetch_report_from_url(input: String) -> Result<RemoteReport, String> {
    let resolved_url = resolve_spark_report_url(&input)?;
    let response = reqwest::Client::new()
        .get(&resolved_url)
        .header(USER_AGENT, "BroKnowMySparkAnalyzer/0.1")
        .header(ACCEPT, "application/x-spark-sampler, application/x-spark-health, application/x-spark-heap, application/octet-stream, */*")
        .send()
        .await
        .map_err(|error| format!("拉取报告失败: {error}"))?;

    if !response.status().is_success() {
        return Err(format!("远程服务返回 HTTP {}", response.status()));
    }

    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("读取报告内容失败: {error}"))?;

    Ok(RemoteReport {
        bytes_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        content_type,
        resolved_url,
    })
}

#[tauri::command]
async fn call_ai(request: AiRequest) -> Result<String, String> {
    post_chat(
        &request.config,
        vec![
            AiMessage {
                role: "system".to_string(),
                content: "你是 Minecraft 服务端/整合包性能诊断助手。只基于用户提供的 spark 报告摘要判断，给出优先级、证据和下一步验证办法。".to_string(),
            },
            AiMessage {
                role: "user".to_string(),
                content: request.prompt,
            },
        ],
        None,
    )
    .await
}

#[tauri::command]
async fn call_ai_chat(request: AiChatRequest) -> Result<String, String> {
    post_chat(&request.config, request.messages, None).await
}

#[tauri::command]
async fn test_ai_connection(config: AiConfig) -> Result<String, String> {
    match post_chat(
        &config,
        vec![
            AiMessage {
                role: "system".to_string(),
                content: "You are a connectivity probe. Reply with exactly: OK".to_string(),
            },
            AiMessage {
                role: "user".to_string(),
                content: "ping".to_string(),
            },
        ],
        Some(64),
    )
    .await
    {
        Ok(content) => Ok(content),
        Err(error) if error.starts_with("AI 响应中没有 message.content:") => {
            Ok("OK (HTTP 200, 响应正文为空但服务可连通)".to_string())
        }
        Err(error) => Err(error),
    }
}

#[tauri::command]
async fn list_ai_models(config: AiConfig) -> Result<Vec<AiModelInfo>, String> {
    let base_url = config.base_url.trim().trim_end_matches('/');
    if base_url.is_empty() {
        return Err("Base URL 不能为空".to_string());
    }
    if config.api_key.trim().is_empty() {
        return Err("API Key 不能为空".to_string());
    }

    let endpoint = format!("{base_url}/models");
    let response = reqwest::Client::new()
        .get(endpoint)
        .header(USER_AGENT, "BroKnowMySparkAnalyzer/0.1")
        .header(AUTHORIZATION, format!("Bearer {}", config.api_key.trim()))
        .send()
        .await
        .map_err(|error| format!("获取模型列表失败: {error}"))?;

    let status = response.status();
    let value: serde_json::Value = response
        .json()
        .await
        .map_err(|error| format!("解析模型列表失败: {error}"))?;

    if !status.is_success() {
        return Err(format!("模型列表返回 HTTP {status}: {value}"));
    }

    let mut models: Vec<AiModelInfo> = value
        .pointer("/data")
        .and_then(|data| data.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    item.get("id")
                        .or_else(|| item.get("name"))
                        .and_then(|id| id.as_str())
                        .map(|id| AiModelInfo { id: id.to_string() })
                })
                .collect()
        })
        .unwrap_or_default();

    models.sort_by(|left, right| left.id.cmp(&right.id));
    models.dedup_by(|left, right| left.id == right.id);
    Ok(models)
}

#[tauri::command]
fn save_export_file(request: SaveExportRequest) -> Result<(), String> {
    let path = PathBuf::from(request.path);
    if path.as_os_str().is_empty() {
        return Err("保存路径不能为空".to_string());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(request.bytes_base64)
        .map_err(|error| format!("导出内容解码失败: {error}"))?;
    fs::write(&path, bytes).map_err(|error| format!("写入文件失败: {error}"))
}

async fn post_chat(
    config: &AiConfig,
    messages: Vec<AiMessage>,
    max_tokens: Option<u32>,
) -> Result<String, String> {
    let base_url = config.base_url.trim().trim_end_matches('/');
    if base_url.is_empty() {
        return Err("Base URL 不能为空".to_string());
    }
    if config.api_key.trim().is_empty() {
        return Err("API Key 不能为空".to_string());
    }
    if config.model.trim().is_empty() {
        return Err("Model 不能为空".to_string());
    }

    let endpoint = format!("{base_url}/chat/completions");
    let mut body = json!({
        "model": config.model,
        "temperature": config.temperature,
        "messages": messages
    });
    if let Some(max_tokens) = max_tokens {
        body["max_tokens"] = json!(max_tokens);
    }

    let response = reqwest::Client::new()
        .post(endpoint)
        .header(USER_AGENT, "BroKnowMySparkAnalyzer/0.1")
        .header(AUTHORIZATION, format!("Bearer {}", config.api_key.trim()))
        .json(&body)
        .send()
        .await
        .map_err(|error| format!("AI 请求失败: {error}"))?;

    let status = response.status();
    let value: serde_json::Value = response
        .json()
        .await
        .map_err(|error| format!("解析 AI 响应失败: {error}"))?;

    if !status.is_success() {
        return Err(format!("AI 返回 HTTP {status}: {value}"));
    }

    value
        .pointer("/choices/0/message/content")
        .and_then(|content| content.as_str())
        .map(|content| content.trim().to_string())
        .filter(|content| !content.is_empty())
        .ok_or_else(|| format!("AI 响应中没有 message.content: {value}"))
}

fn resolve_spark_report_url(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("链接或 key 不能为空".to_string());
    }

    if let Ok(url) = Url::parse(trimmed) {
        let host = url.host_str().unwrap_or_default();
        if host.contains("spark-usercontent.lucko.me") {
            return Ok(url.to_string());
        }

        if host.contains("spark.lucko.me") {
            let key = url
                .path_segments()
                .and_then(|segments| {
                    segments
                        .filter(|segment| !segment.is_empty())
                        .filter(|segment| *segment != "viewer" && *segment != "profile")
                        .last()
                        .map(|segment| segment.to_string())
                })
                .or_else(|| {
                    url.query_pairs()
                        .find(|(name, _)| name == "id" || name == "key")
                        .map(|(_, value)| value.into_owned())
                })
                .ok_or_else(|| "无法从 spark viewer 链接解析报告 key".to_string())?;
            return Ok(format!("https://spark-usercontent.lucko.me/{key}"));
        }

        return Ok(url.to_string());
    }

    Ok(format!(
        "https://spark-usercontent.lucko.me/{}",
        trimmed.trim_matches('/')
    ))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            fetch_report_from_url,
            call_ai,
            call_ai_chat,
            test_ai_connection,
            list_ai_models,
            save_export_file
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
