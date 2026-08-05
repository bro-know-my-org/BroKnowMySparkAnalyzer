use base64::Engine;
use reqwest::header::{ACCEPT, CONTENT_TYPE, USER_AGENT};
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;
use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime,
};
use url::Url;

mod state;

use state::{AnalyzerState, LoadedReport};

const MAX_REPORT_BYTES: usize = 256 * 1024 * 1024;

#[derive(Debug)]
struct RemoteReport {
    bytes: Vec<u8>,
    content_type: String,
    resolved_url: String,
}

#[derive(Debug, Deserialize)]
struct SaveExportRequest {
    path: String,
    bytes_base64: String,
}

#[derive(Debug, Deserialize)]
struct LoadReportRequest {
    bytes_base64: String,
    source: String,
    #[serde(default)]
    hint: String,
}

#[derive(Debug, Deserialize)]
struct LoadTextReportRequest {
    text: String,
    source: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ExecuteToolRequest {
    report_id: String,
    tool: String,
    #[serde(default)]
    args: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct RunAnalysisRequest {
    report_id: String,
    config: bkmsa_agent::AiConfig,
}

#[derive(Debug, Deserialize)]
struct AskFollowUpRequest {
    report_id: String,
    config: bkmsa_agent::AiConfig,
    traces: Vec<bkmsa_agent::AgentTrace>,
    diagnosis: String,
    #[serde(default)]
    history: Vec<bkmsa_agent::FollowUpMessage>,
    question: String,
}

#[derive(Debug, Deserialize)]
struct ReleaseReportRequest {
    report_id: String,
}

#[derive(Debug, Deserialize)]
struct StoreApiKeyRequest {
    api_key: String,
}

fn api_key_entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(
        "io.github.broknowmyorg.broknowmysparkanalyzer",
        "bkmsa-api-key",
    )
    .map_err(|error| format!("无法打开系统凭据存储: {error}"))
}

#[tauri::command]
fn analyzer_store_api_key(request: StoreApiKeyRequest) -> Result<(), String> {
    if request.api_key.trim().is_empty() {
        return Err("API Key 不能为空".to_string());
    }
    api_key_entry()?
        .set_password(request.api_key.trim())
        .map_err(|error| format!("保存 API Key 到系统凭据存储失败: {error}"))
}

#[tauri::command]
fn analyzer_load_api_key() -> Result<Option<String>, String> {
    let entry = api_key_entry()?;
    match entry.get_password() {
        Ok(secret) => Ok(Some(secret)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(format!("读取系统凭据存储失败: {error}")),
    }
}

#[tauri::command]
fn analyzer_delete_api_key() -> Result<(), String> {
    let entry = api_key_entry()?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(format!("删除系统凭据失败: {error}")),
    }
}

#[tauri::command]
fn analyzer_load_report_bytes(
    request: LoadReportRequest,
    state: tauri::State<'_, AnalyzerState>,
) -> Result<LoadedReport, String> {
    if request.bytes_base64.len() > MAX_REPORT_BYTES.saturating_mul(4) / 3 + 16 {
        return Err("报告超过 256 MiB 限制".to_string());
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(request.bytes_base64)
        .map_err(|error| format!("报告内容 Base64 解码失败: {error}"))?;
    if bytes.len() > MAX_REPORT_BYTES {
        return Err("报告超过 256 MiB 限制".to_string());
    }
    let report = bkmsa_core::parse_report_bytes(&bytes, request.source, request.hint)
        .map_err(|error| error.to_string())?;
    state.insert(report)
}

#[tauri::command]
fn analyzer_load_text_report(
    request: LoadTextReportRequest,
    state: tauri::State<'_, AnalyzerState>,
) -> Result<LoadedReport, String> {
    state.insert(
        bkmsa_core::parse_text_report(
            request.text,
            request.source.unwrap_or_else(|| "pasted text".to_string()),
        )
        .map_err(|error| error.to_string())?,
    )
}

#[tauri::command]
async fn analyzer_fetch_report(
    input: String,
    state: tauri::State<'_, AnalyzerState>,
) -> Result<LoadedReport, String> {
    let remote = fetch_remote_report(&input).await?;
    let hint = format!("{} {}", remote.content_type, remote.resolved_url);
    let report = bkmsa_core::parse_report_bytes(&remote.bytes, remote.resolved_url, hint)
        .map_err(|error| error.to_string())?;
    state.insert(report)
}

#[tauri::command]
fn analyzer_execute_tool(
    request: ExecuteToolRequest,
    state: tauri::State<'_, AnalyzerState>,
) -> Result<serde_json::Value, String> {
    let report = state.get(&request.report_id)?;
    bkmsa_core::execute_tool(&report, &request.tool, request.args)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn analyzer_release_report(
    request: ReleaseReportRequest,
    state: tauri::State<'_, AnalyzerState>,
) -> Result<bool, String> {
    state.remove(&request.report_id)
}

#[tauri::command]
async fn analyzer_run_analysis(
    request: RunAnalysisRequest,
    state: tauri::State<'_, AnalyzerState>,
) -> Result<bkmsa_agent::AgentResult, String> {
    let report = state.get(&request.report_id)?;
    let client =
        bkmsa_agent::OpenAiClient::new(request.config).map_err(|error| error.to_string())?;
    let (analysis_id, cancellation) = state.begin_analysis(&request.report_id)?;
    let result = tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err("分析已中止".to_string()),
        result = bkmsa_agent::run_analysis(report.as_ref(), &client, bkmsa_agent::AgentOptions::default()) => {
            result.map_err(|error| error.to_string())
        }
    };
    state.finish_analysis(&request.report_id, analysis_id)?;
    result
}

#[tauri::command]
fn analyzer_cancel_analysis(
    request: ReleaseReportRequest,
    state: tauri::State<'_, AnalyzerState>,
) -> Result<bool, String> {
    state.cancel_analysis(&request.report_id)
}

#[tauri::command]
async fn analyzer_ask_follow_up(
    request: AskFollowUpRequest,
    state: tauri::State<'_, AnalyzerState>,
) -> Result<String, String> {
    use bkmsa_agent::ToolExecutor;

    let report = state.get(&request.report_id)?;
    let client =
        bkmsa_agent::OpenAiClient::new(request.config).map_err(|error| error.to_string())?;
    bkmsa_agent::ask_follow_up(
        &report.context(),
        &client,
        &request.traces,
        &request.diagnosis,
        &request.history,
        &request.question,
    )
    .await
    .map_err(|error| error.to_string())
}

#[tauri::command]
async fn analyzer_test_ai_connection(config: bkmsa_agent::AiConfig) -> Result<String, String> {
    let client = bkmsa_agent::OpenAiClient::new(config).map_err(|error| error.to_string())?;
    client
        .test_connection()
        .await
        .map_err(|error| error.to_string())?;
    Ok("OK".to_string())
}

#[tauri::command]
async fn analyzer_list_ai_models(
    config: bkmsa_agent::AiConfig,
) -> Result<Vec<bkmsa_agent::ModelInfo>, String> {
    let client = bkmsa_agent::OpenAiClient::new(config).map_err(|error| error.to_string())?;
    client
        .list_models()
        .await
        .map_err(|error| error.to_string())
}

async fn fetch_remote_report(input: &str) -> Result<RemoteReport, String> {
    let resolved_url = resolve_spark_report_url(input)?;
    let response = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if allowed_report_url(attempt.url()) {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .build()
        .map_err(|error| format!("创建下载客户端失败: {error}"))?
        .get(&resolved_url)
        .header(USER_AGENT, "BroKnowMySparkAnalyzer/0.1")
        .header(ACCEPT, "application/x-spark-sampler, application/x-spark-health, application/x-spark-heap, application/octet-stream, */*")
        .send()
        .await
        .map_err(|error| format!("拉取报告失败: {error}"))?;

    if !response.status().is_success() {
        return Err(format!("远程服务返回 HTTP {}", response.status()));
    }

    if response
        .content_length()
        .is_some_and(|length| length > MAX_REPORT_BYTES as u64)
    {
        return Err("远程报告超过 256 MiB 限制".to_string());
    }

    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let final_url = response.url().clone();
    let mut response = response;
    let mut bytes = Vec::with_capacity(
        response
            .content_length()
            .unwrap_or_default()
            .min(MAX_REPORT_BYTES as u64) as usize,
    );
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("读取报告内容失败: {error}"))?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_REPORT_BYTES {
            return Err("远程报告超过 256 MiB 限制".to_string());
        }
        bytes.extend_from_slice(&chunk);
    }

    Ok(RemoteReport {
        bytes,
        content_type,
        resolved_url: redact_url(final_url),
    })
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

fn resolve_spark_report_url(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("链接或 key 不能为空".to_string());
    }

    if let Ok(mut url) = Url::parse(trimmed) {
        let host = url.host_str().unwrap_or_default();
        if host == "spark-usercontent.lucko.me" {
            if url.port_or_known_default() != Some(443) {
                return Err("spark 报告 URL 必须使用默认 HTTPS 端口".into());
            }
            url.set_scheme("https")
                .map_err(|_| "无法规范化报告链接".to_string())?;
            if !url.username().is_empty() || url.password().is_some() {
                return Err("报告链接不能包含用户名或密码".to_string());
            }
            return Ok(url.to_string());
        }

        if host == "spark.lucko.me" {
            let key = url
                .path_segments()
                .and_then(|mut segments| {
                    segments
                        .rfind(|segment| {
                            !segment.is_empty() && *segment != "viewer" && *segment != "profile"
                        })
                        .map(|segment| {
                            percent_encoding::percent_decode_str(segment)
                                .decode_utf8_lossy()
                                .into_owned()
                        })
                })
                .or_else(|| {
                    url.query_pairs()
                        .find(|(name, _)| name == "id" || name == "key")
                        .map(|(_, value)| value.into_owned())
                })
                .ok_or_else(|| "无法从 spark viewer 链接解析报告 key".to_string())?;
            return Ok(content_url(&key));
        }

        return Err(format!("不支持的报告主机: {host}"));
    }

    Ok(content_url(trimmed.trim_matches('/')))
}

fn content_url(key: &str) -> String {
    let mut url =
        Url::parse("https://spark-usercontent.lucko.me/").expect("static spark content URL");
    url.path_segments_mut()
        .expect("spark content URL is hierarchical")
        .push(key);
    url.into()
}

fn allowed_report_url(url: &Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some("spark-usercontent.lucko.me")
        && url.port_or_known_default() == Some(443)
}

fn redact_url(mut url: Url) -> String {
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    url.into()
}

/// Initializes the reusable native backend used by every Tauri host.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("bkmsa")
        .invoke_handler(tauri::generate_handler![
            analyzer_load_report_bytes,
            analyzer_load_text_report,
            analyzer_fetch_report,
            analyzer_execute_tool,
            analyzer_release_report,
            analyzer_run_analysis,
            analyzer_cancel_analysis,
            analyzer_ask_follow_up,
            analyzer_test_ai_connection,
            analyzer_list_ai_models,
            analyzer_store_api_key,
            analyzer_load_api_key,
            analyzer_delete_api_key,
            save_export_file
        ])
        .setup(|app, _api| {
            app.manage(AnalyzerState::default());
            Ok(())
        })
        .build()
}
