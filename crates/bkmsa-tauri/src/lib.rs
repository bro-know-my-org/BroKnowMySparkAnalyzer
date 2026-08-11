use base64::Engine;
use reqwest::header::{ACCEPT, CONTENT_TYPE, USER_AGENT};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime,
};
use tauri_plugin_dialog::DialogExt;
use url::Url;

mod state;

use state::{AnalyzerState, LoadedReport};

const MAX_REPORT_BYTES: usize = 64 * 1024 * 1024;
const MAX_EXPORT_BYTES: usize = 32 * 1024 * 1024;
const MAX_SAVE_DIALOG_OPTIONS_BYTES: usize = 16 * 1024;
const STORED_API_KEY_HANDLE: &str = "__BKMSA_STORED_API_KEY__";
static CREDENTIAL_OPERATION_LOCK: OnceLock<Arc<tokio::sync::Mutex<()>>> = OnceLock::new();

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
#[serde(rename_all = "camelCase")]
struct SaveDialogOptions {
    default_path: String,
    #[serde(default)]
    filters: Vec<SaveDialogFilter>,
}

#[derive(Debug, Deserialize)]
struct SaveDialogFilter {
    name: String,
    extensions: Vec<String>,
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
    base_url: String,
}

#[derive(Debug, Deserialize)]
struct StoredAiCredential {
    api_key: String,
    base_url: String,
}

struct AnalysisCleanup<'a> {
    state: &'a AnalyzerState,
    report_id: String,
    analysis_id: u64,
    active: bool,
}

impl<'a> AnalysisCleanup<'a> {
    fn new(state: &'a AnalyzerState, report_id: String, analysis_id: u64) -> Self {
        Self {
            state,
            report_id,
            analysis_id,
            active: true,
        }
    }

    fn finish(mut self) -> Result<(), String> {
        self.state
            .finish_analysis(&self.report_id, self.analysis_id)?;
        self.active = false;
        Ok(())
    }
}

impl Drop for AnalysisCleanup<'_> {
    fn drop(&mut self) {
        if self.active {
            let _ = self
                .state
                .finish_analysis(&self.report_id, self.analysis_id);
        }
    }
}

fn credential_entry(account: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new("io.github.broknowmyorg.broknowmysparkanalyzer", account)
        .map_err(|error| format!("无法打开系统凭据存储: {error}"))
}

fn api_key_entry() -> Result<keyring::Entry, String> {
    credential_entry("bkmsa-ai-credentials")
}

fn legacy_api_key_entry() -> Result<keyring::Entry, String> {
    credential_entry("bkmsa-api-key")
}

fn credential_operation_lock() -> Arc<tokio::sync::Mutex<()>> {
    CREDENTIAL_OPERATION_LOCK
        .get_or_init(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

async fn run_credential_operation<T, F>(failure: &str, operation: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    let guard = credential_operation_lock().lock_owned().await;
    tokio::task::spawn_blocking(move || {
        let _guard = guard;
        operation()
    })
    .await
    .map_err(|error| format!("{failure}: {error}"))?
}

fn store_credential_locked(api_key: &str, base_url: &str) -> Result<(), String> {
    let credential = serde_json::json!({
        "api_key": api_key,
        "base_url": base_url,
    })
    .to_string();
    let entry = api_key_entry()?;
    entry
        .set_password(&credential)
        .map_err(|error| format!("保存 AI 凭据到系统凭据存储失败: {error}"))?;
    legacy_api_key_entry().and_then(|legacy| match legacy.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(format!("清理旧版 API Key 失败: {error}")),
    })
}

fn store_api_key(request: StoreApiKeyRequest) -> Result<(), String> {
    if request.api_key.trim().is_empty() {
        return Err("API Key 不能为空".to_string());
    }
    if request.api_key == STORED_API_KEY_HANDLE {
        let record = api_key_entry()?
            .get_password()
            .map_err(|error| format!("读取系统凭据存储失败: {error}"))?;
        let credential: StoredAiCredential = serde_json::from_str(&record)
            .map_err(|_| "系统凭据存储中的 AI 凭据格式无效，请重新输入密钥".to_string())?;
        if credential.base_url.trim_end_matches('/') != request.base_url.trim_end_matches('/') {
            return Err("API 服务地址已变化，请重新输入并保存密钥".to_string());
        }
        return Ok(());
    }
    let validated = bkmsa_agent::AiConfig::new(
        request.base_url.trim(),
        request.api_key.trim(),
        "credential-validation",
        0.0,
    )
    .map_err(|error| error.to_string())?;
    store_credential_locked(validated.api_key(), validated.base_url())
}

#[tauri::command]
async fn analyzer_store_api_key(request: StoreApiKeyRequest) -> Result<(), String> {
    run_credential_operation("保存 AI 凭据任务失败", move || {
        store_api_key(request)
    })
    .await
}

fn load_api_key(base_url: Option<String>) -> Result<Option<String>, String> {
    let entry = api_key_entry()?;
    match entry.get_password() {
        Ok(record) => match serde_json::from_str::<StoredAiCredential>(&record) {
            Ok(credential)
                if !credential.api_key.trim().is_empty()
                    && !credential.base_url.trim().is_empty()
                    && base_url.as_deref().is_none_or(|requested| {
                        requested.trim_end_matches('/') == credential.base_url.trim_end_matches('/')
                    }) =>
            {
                Ok(Some(STORED_API_KEY_HANDLE.to_string()))
            }
            _ => Ok(None),
        },
        Err(keyring::Error::NoEntry) => match legacy_api_key_entry()?.get_password() {
            Ok(api_key) if !api_key.trim().is_empty() => Err(
                "检测到旧版 API Key；请重新输入并保存，以将其绑定到当前 API 服务地址".to_string(),
            ),
            Ok(_) | Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(format!("读取旧版 API Key 失败: {error}")),
        },
        Err(error) => Err(format!("读取系统凭据存储失败: {error}")),
    }
}

#[tauri::command]
async fn analyzer_load_api_key(base_url: Option<String>) -> Result<Option<String>, String> {
    run_credential_operation("读取 AI 凭据任务失败", move || {
        load_api_key(base_url)
    })
    .await
}

fn resolve_ai_config_blocking(
    config: bkmsa_agent::AiConfig,
) -> Result<bkmsa_agent::AiConfig, String> {
    if config.api_key() != STORED_API_KEY_HANDLE {
        return Ok(config);
    }
    let record = api_key_entry()?
        .get_password()
        .map_err(|error| format!("读取系统凭据存储失败: {error}"))?;
    let credential: StoredAiCredential = serde_json::from_str(&record)
        .map_err(|_| "系统凭据存储中的 AI 凭据格式无效，请重新保存配置".to_string())?;
    if config.base_url().trim_end_matches('/') != credential.base_url.trim_end_matches('/') {
        return Err("已保存的 API Key 与当前服务地址不匹配，请重新保存配置".to_string());
    }
    config
        .with_api_key(credential.api_key)
        .map_err(|error| error.to_string())
}

async fn resolve_ai_config(config: bkmsa_agent::AiConfig) -> Result<bkmsa_agent::AiConfig, String> {
    run_credential_operation("读取 AI 凭据任务失败", move || {
        resolve_ai_config_blocking(config)
    })
    .await
}

#[tauri::command]
async fn analyzer_delete_api_key() -> Result<(), String> {
    run_credential_operation("删除 AI 凭据任务失败", || {
        let mut errors = Vec::new();
        for entry in [api_key_entry(), legacy_api_key_entry()] {
            match entry {
                Ok(entry) => match entry.delete_credential() {
                    Ok(()) | Err(keyring::Error::NoEntry) => {}
                    Err(error) => errors.push(error.to_string()),
                },
                Err(error) => errors.push(error),
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(format!("删除系统凭据失败: {}", errors.join("；")))
        }
    })
    .await
}

#[tauri::command]
async fn analyzer_load_report_bytes(
    request: LoadReportRequest,
    state: tauri::State<'_, AnalyzerState>,
) -> Result<LoadedReport, String> {
    let permit = state.try_acquire_report_load_permit()?;
    let report = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        if request.bytes_base64.len() > MAX_REPORT_BYTES.saturating_mul(4) / 3 + 16 {
            return Err("报告超过 64 MiB 限制".to_string());
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(request.bytes_base64)
            .map_err(|error| format!("报告内容 Base64 解码失败: {error}"))?;
        if bytes.len() > MAX_REPORT_BYTES {
            return Err("报告超过 64 MiB 限制".to_string());
        }
        bkmsa_core::parse_report_bytes(&bytes, request.source, request.hint)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("报告解析任务失败: {error}"))??;
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
    let _permit = state.try_acquire_report_load_permit()?;
    let remote = fetch_remote_report(&input).await?;
    let hint = format!("{} {}", remote.content_type, remote.resolved_url);
    let report = tokio::task::spawn_blocking(move || {
        bkmsa_core::parse_report_bytes(&remote.bytes, remote.resolved_url, hint)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("报告解析任务失败: {error}"))??;
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
    let client = bkmsa_agent::OpenAiClient::new(resolve_ai_config(request.config).await?)
        .map_err(|error| error.to_string())?;
    let (analysis_id, cancellation) = state.begin_analysis(&request.report_id)?;
    let cleanup = AnalysisCleanup::new(&state, request.report_id.clone(), analysis_id);
    let result = tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err("分析已中止".to_string()),
        result = bkmsa_agent::run_analysis(report.as_ref(), &client, bkmsa_agent::AgentOptions::default()) => {
            result.map_err(|error| error.to_string())
        }
    };
    cleanup.finish()?;
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
    let client = bkmsa_agent::OpenAiClient::new(resolve_ai_config(request.config).await?)
        .map_err(|error| error.to_string())?;
    let (analysis_id, cancellation) = state.begin_analysis(&request.report_id)?;
    let cleanup = AnalysisCleanup::new(&state, request.report_id.clone(), analysis_id);
    let context = report.context();
    let result = tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err("追问已中止".to_string()),
        result = bkmsa_agent::ask_follow_up(
            &context,
            &client,
            &request.traces,
            &request.diagnosis,
            &request.history,
            &request.question,
        ) => result.map_err(|error| error.to_string()),
    };
    cleanup.finish()?;
    result
}

#[tauri::command]
async fn analyzer_test_ai_connection(config: bkmsa_agent::AiConfig) -> Result<String, String> {
    let client = bkmsa_agent::OpenAiClient::new(resolve_ai_config(config).await?)
        .map_err(|error| error.to_string())?;
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
    let client = bkmsa_agent::OpenAiClient::new(resolve_ai_config(config).await?)
        .map_err(|error| error.to_string())?;
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
            if attempt.previous().len() < 10 && allowed_report_url(attempt.url()) {
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
        return Err("远程报告超过 64 MiB 限制".to_string());
    }

    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();
    let final_url = response.url().clone();
    let mut response = response;
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("读取报告内容失败: {error}"))?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_REPORT_BYTES {
            return Err("远程报告超过 64 MiB 限制".to_string());
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
async fn save_export_file<R: Runtime>(
    request: SaveExportRequest,
    app: tauri::AppHandle<R>,
) -> Result<Option<String>, String> {
    if request.bytes_base64.len() > MAX_EXPORT_BYTES.saturating_mul(4) / 3 + 16 {
        return Err("导出内容超过 32 MiB 限制".to_string());
    }
    if request.path.len() > MAX_SAVE_DIALOG_OPTIONS_BYTES {
        return Err("原生保存对话框参数超过 16 KiB 限制".to_string());
    }
    let options: SaveDialogOptions =
        serde_json::from_str(&request.path).map_err(|_| "无效的原生保存对话框参数".to_string())?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(request.bytes_base64)
        .map_err(|error| format!("导出内容解码失败: {error}"))?;
    if bytes.len() > MAX_EXPORT_BYTES {
        return Err("导出内容超过 32 MiB 限制".to_string());
    }
    let file_name = PathBuf::from(&options.default_path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("bkmsa-export.md")
        .to_string();
    let dialog_path = tokio::task::spawn_blocking(move || {
        let mut dialog = app.dialog().file().set_file_name(file_name);
        for filter in options.filters.into_iter().take(8) {
            let extensions = filter
                .extensions
                .iter()
                .take(16)
                .map(String::as_str)
                .collect::<Vec<_>>();
            dialog = dialog.add_filter(
                filter.name.chars().take(80).collect::<String>(),
                &extensions,
            );
        }
        dialog.blocking_save_file()
    })
    .await
    .map_err(|error| format!("保存对话框任务失败: {error}"))?;
    let Some(path) = dialog_path else {
        return Ok(None);
    };
    let path = path
        .into_path()
        .map_err(|_| "保存目标不是本地文件路径".to_string())?;
    let write_path = path.clone();
    tokio::task::spawn_blocking(move || std::fs::write(&write_path, bytes))
        .await
        .map_err(|error| format!("文件写入任务失败: {error}"))?
        .map_err(|error| format!("写入文件失败: {error}"))?;
    Ok(Some(path.display().to_string()))
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
