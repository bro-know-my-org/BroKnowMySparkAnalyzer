use bkmsa_core::{
    execute_tool, parse_report_bytes, parse_text_report, report_tool_descriptions, Report,
};
use serde::Serialize;
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, VecDeque},
    sync::Arc,
};
use wasm_bindgen::prelude::*;

const MAX_TEXT_REPORT_BYTES: usize = 16 * 1024 * 1024;
const MAX_REPORT_SESSIONS: usize = 8;

fn js_error(error: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&error.to_string())
}

fn to_js_value<T: Serialize>(value: &T) -> Result<JsValue, JsValue> {
    value
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(js_error)
}

/// Parses a spark protobuf report and returns the canonical Rust report document as JSON.
///
/// JSON is used deliberately at this boundary: it keeps the Vue layer free of report logic
/// while avoiding a second TypeScript model that can drift away from `bkmsa-core`.
#[wasm_bindgen]
pub fn parse_report(bytes: &[u8], source: &str, hint: &str) -> Result<String, JsValue> {
    let report = parse_report_bytes(bytes, source, hint).map_err(js_error)?;
    serde_json::to_string(&report).map_err(js_error)
}

#[wasm_bindgen]
pub fn parse_text(text: &str, source: &str) -> Result<String, JsValue> {
    ensure_text_size(text)?;
    serde_json::to_string(&parse_text_report(text, source).map_err(js_error)?).map_err(js_error)
}

#[wasm_bindgen]
pub fn execute_report_tool(
    report_json: &str,
    tool: &str,
    args_json: &str,
) -> Result<String, JsValue> {
    let report: Report = serde_json::from_str(report_json).map_err(js_error)?;
    let args = if args_json.trim().is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(args_json).map_err(js_error)?
    };
    let result = execute_tool(&report, tool, args).map_err(js_error)?;
    serde_json::to_string(&result).map_err(js_error)
}

#[wasm_bindgen]
pub fn report_tools() -> Result<String, JsValue> {
    serde_json::to_string(&report_tool_descriptions()).map_err(js_error)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LoadedReport<'a> {
    report_id: &'a str,
    kind: &'static str,
    source: &'a str,
    summary: &'a bkmsa_core::ReportSummary,
}

/// Stateful browser facade matching the desktop adapter contract.
/// Large decoded reports remain inside WASM; Vue only receives an opaque report id and summary.
#[wasm_bindgen]
pub struct Analyzer {
    next_report_id: Cell<u64>,
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    next_analysis_id: Cell<u64>,
    reports: RefCell<HashMap<String, Arc<Report>>>,
    report_order: RefCell<VecDeque<String>>,
    analysis_runs: RefCell<HashMap<String, AnalysisRun>>,
}

#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
struct AnalysisRun {
    id: u64,
    handle: futures::future::AbortHandle,
}

#[wasm_bindgen]
impl Analyzer {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            next_report_id: Cell::new(1),
            next_analysis_id: Cell::new(1),
            reports: RefCell::new(HashMap::new()),
            report_order: RefCell::new(VecDeque::new()),
            analysis_runs: RefCell::new(HashMap::new()),
        }
    }

    #[wasm_bindgen(js_name = loadReportBytes)]
    pub fn load_report_bytes(
        &self,
        bytes: &[u8],
        source: &str,
        hint: &str,
    ) -> Result<JsValue, JsValue> {
        let report = parse_report_bytes(bytes, source, hint).map_err(js_error)?;
        self.insert_report(report)
    }

    #[wasm_bindgen(js_name = loadTextReport)]
    pub fn load_text_report(&self, text: &str, source: Option<String>) -> Result<JsValue, JsValue> {
        ensure_text_size(text)?;
        self.insert_report(
            parse_text_report(text, source.unwrap_or_else(|| "pasted text".to_string()))
                .map_err(js_error)?,
        )
    }

    #[wasm_bindgen(js_name = executeTool)]
    pub fn execute_report_tool(
        &self,
        report_id: &str,
        tool: &str,
        args: JsValue,
    ) -> Result<JsValue, JsValue> {
        let args = if args.is_null() || args.is_undefined() {
            serde_json::json!({})
        } else {
            serde_wasm_bindgen::from_value(args).map_err(js_error)?
        };
        let reports = self.reports.borrow();
        let report = reports
            .get(report_id)
            .ok_or_else(|| js_error(format!("报告会话不存在或已释放: {report_id}")))?;
        let result = execute_tool(report, tool, args).map_err(js_error)?;
        to_js_value(&result)
    }

    #[wasm_bindgen(js_name = releaseReport)]
    pub fn release_report(&self, report_id: &str) {
        self.cancel_analysis(report_id);
        self.reports.borrow_mut().remove(report_id);
        self.report_order.borrow_mut().retain(|id| id != report_id);
    }

    #[wasm_bindgen(js_name = cancelAnalysis)]
    pub fn cancel_analysis(&self, report_id: &str) -> bool {
        if let Some(run) = self.analysis_runs.borrow_mut().remove(report_id) {
            run.handle.abort();
            return true;
        }
        false
    }

    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen(js_name = runAnalysis)]
    pub async fn run_analysis(
        &self,
        report_id: String,
        config: JsValue,
    ) -> Result<JsValue, JsValue> {
        let report = self.report_clone(&report_id)?;
        let config: bkmsa_agent::AiConfig =
            serde_wasm_bindgen::from_value(config).map_err(js_error)?;
        config.validate().map_err(js_error)?;
        let client = BrowserChatClient { config };
        let (handle, registration) = futures::future::AbortHandle::new_pair();
        let run_id = self.next_analysis_id.get();
        self.next_analysis_id.set(run_id.wrapping_add(1));
        if let Some(previous) = self
            .analysis_runs
            .borrow_mut()
            .insert(report_id.clone(), AnalysisRun { id: run_id, handle })
        {
            previous.handle.abort();
        }
        let result = futures::future::Abortable::new(
            bkmsa_agent::run_analysis(
                report.as_ref(),
                &client,
                bkmsa_agent::AgentOptions::default(),
            ),
            registration,
        )
        .await;
        let mut runs = self.analysis_runs.borrow_mut();
        if runs.get(&report_id).is_some_and(|run| run.id == run_id) {
            runs.remove(&report_id);
        }
        drop(runs);
        let result = result
            .map_err(|_| js_error("分析已中止"))?
            .map_err(js_error)?;
        to_js_value(&result)
    }

    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen(js_name = askFollowUp)]
    pub async fn ask_follow_up(
        &self,
        report_id: String,
        config: JsValue,
        traces: JsValue,
        diagnosis: String,
        history: JsValue,
        question: String,
    ) -> Result<String, JsValue> {
        use bkmsa_agent::ToolExecutor;

        let report = self.report_clone(&report_id)?;
        let config: bkmsa_agent::AiConfig =
            serde_wasm_bindgen::from_value(config).map_err(js_error)?;
        config.validate().map_err(js_error)?;
        let traces: Vec<bkmsa_agent::AgentTrace> =
            serde_wasm_bindgen::from_value(traces).map_err(js_error)?;
        let history: Vec<bkmsa_agent::FollowUpMessage> =
            serde_wasm_bindgen::from_value(history).map_err(js_error)?;
        let client = BrowserChatClient { config };
        let (handle, registration) = futures::future::AbortHandle::new_pair();
        let run_id = self.next_analysis_id.get();
        self.next_analysis_id.set(run_id.wrapping_add(1));
        if let Some(previous) = self
            .analysis_runs
            .borrow_mut()
            .insert(report_id.clone(), AnalysisRun { id: run_id, handle })
        {
            previous.handle.abort();
        }
        let result = futures::future::Abortable::new(
            bkmsa_agent::ask_follow_up(
                &report.context(),
                &client,
                &traces,
                &diagnosis,
                &history,
                &question,
            ),
            registration,
        )
        .await;
        let mut runs = self.analysis_runs.borrow_mut();
        if runs.get(&report_id).is_some_and(|run| run.id == run_id) {
            runs.remove(&report_id);
        }
        drop(runs);
        result
            .map_err(|_| js_error("追问已中止"))?
            .map_err(js_error)
    }

    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen(js_name = testAiConnection)]
    pub async fn test_ai_connection(&self, config: JsValue) -> Result<String, JsValue> {
        let config: bkmsa_agent::AiConfig =
            serde_wasm_bindgen::from_value(config).map_err(js_error)?;
        config.validate().map_err(js_error)?;
        let client = BrowserChatClient { config };
        bkmsa_agent::ChatClient::chat(
            &client,
            &[
                bkmsa_agent::ChatMessage::system(
                    "You are a connectivity probe. Reply with exactly: OK",
                ),
                bkmsa_agent::ChatMessage::user("ping"),
            ],
        )
        .await
        .map_err(js_error)
    }

    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen(js_name = listAiModels)]
    pub async fn list_ai_models(&self, config: JsValue) -> Result<JsValue, JsValue> {
        let config: bkmsa_agent::AiConfig =
            serde_wasm_bindgen::from_value(config).map_err(js_error)?;
        config.validate().map_err(js_error)?;
        let models = BrowserChatClient { config }
            .list_models()
            .await
            .map_err(js_error)?;
        to_js_value(&models)
    }
}

impl Analyzer {
    fn insert_report(&self, report: Report) -> Result<JsValue, JsValue> {
        let report_id = format!("wasm-report-{}", self.next_report_id.get());
        self.next_report_id.set(self.next_report_id.get() + 1);
        let loaded = LoadedReport {
            report_id: &report_id,
            kind: report.kind.as_str(),
            source: &report.source,
            summary: &report.summary,
        };
        let value = to_js_value(&loaded)?;
        self.reports
            .borrow_mut()
            .insert(report_id.clone(), Arc::new(report));
        self.report_order.borrow_mut().push_back(report_id);
        while self.reports.borrow().len() > MAX_REPORT_SESSIONS {
            let Some(evicted_id) = self.report_order.borrow_mut().pop_front() else {
                break;
            };
            self.cancel_analysis(&evicted_id);
            self.reports.borrow_mut().remove(&evicted_id);
        }
        Ok(value)
    }

    #[cfg(target_arch = "wasm32")]
    fn report_clone(&self, report_id: &str) -> Result<Arc<Report>, JsValue> {
        self.reports
            .borrow()
            .get(report_id)
            .cloned()
            .ok_or_else(|| js_error(format!("报告会话不存在或已释放: {report_id}")))
    }
}

fn ensure_text_size(text: &str) -> Result<(), JsValue> {
    if text.len() > MAX_TEXT_REPORT_BYTES {
        Err(js_error("文本报告超过 16 MiB 限制"))
    } else {
        Ok(())
    }
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_arch = "wasm32")]
struct BrowserChatClient {
    config: bkmsa_agent::AiConfig,
}

#[cfg(target_arch = "wasm32")]
impl BrowserChatClient {
    fn endpoint(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.config.base_url().trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    async fn list_models(&self) -> bkmsa_agent::Result<Vec<bkmsa_agent::ModelInfo>> {
        let controller = web_sys::AbortController::new().map_err(browser_js_error)?;
        let request = gloo_net::http::Request::get(&self.endpoint("models"))
            .redirect(web_sys::RequestRedirect::Error)
            .abort_signal(Some(&controller.signal()))
            .header(
                "Authorization",
                &format!("Bearer {}", self.config.api_key().trim()),
            )
            .build()
            .map_err(browser_request_error)?;
        let response = browser_send(request, controller, self.config.timeout_secs()).await?;
        let status = response.status();
        let body = read_provider_body(&response).await?;
        if !(200..300).contains(&status) {
            return Err(bkmsa_agent::AgentError::Provider {
                status,
                message: body.to_string(),
            });
        }
        let models = body
            .get("data")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| item.get("id").or_else(|| item.get("name")))
            .filter_map(serde_json::Value::as_str)
            .map(|id| bkmsa_agent::ModelInfo { id: id.to_string() })
            .collect();
        Ok(models)
    }
}

#[cfg(target_arch = "wasm32")]
#[async_trait::async_trait(?Send)]
impl bkmsa_agent::ChatClient for BrowserChatClient {
    async fn chat(&self, messages: &[bkmsa_agent::ChatMessage]) -> bkmsa_agent::Result<String> {
        let mut body = serde_json::json!({
            "model": self.config.model(),
            "messages": messages,
        });
        if supports_temperature(self.config.model()) {
            body["temperature"] = serde_json::json!(self.config.temperature());
        }
        let controller = web_sys::AbortController::new().map_err(browser_js_error)?;
        let request = gloo_net::http::Request::post(&self.endpoint("chat/completions"))
            .redirect(web_sys::RequestRedirect::Error)
            .abort_signal(Some(&controller.signal()))
            .header(
                "Authorization",
                &format!("Bearer {}", self.config.api_key().trim()),
            )
            .json(&body)
            .map_err(browser_request_error)?;
        let response = browser_send(request, controller, self.config.timeout_secs()).await?;
        let status = response.status();
        let body = read_provider_body(&response).await?;
        if !(200..300).contains(&status) {
            return Err(bkmsa_agent::AgentError::Provider {
                status,
                message: body.to_string(),
            });
        }
        if let Some(refusal) = body
            .pointer("/choices/0/message/refusal")
            .and_then(serde_json::Value::as_str)
        {
            return Err(bkmsa_agent::AgentError::Refusal(
                refusal.chars().take(1_000).collect(),
            ));
        }
        browser_response_text(&body).ok_or(bkmsa_agent::AgentError::EmptyResponse)
    }
}

#[cfg(target_arch = "wasm32")]
async fn browser_send(
    request: gloo_net::http::Request,
    controller: web_sys::AbortController,
    timeout_secs: u64,
) -> bkmsa_agent::Result<gloo_net::http::Response> {
    use futures::future::{select, Either};

    struct AbortOnDrop(Option<web_sys::AbortController>);
    impl Drop for AbortOnDrop {
        fn drop(&mut self) {
            if let Some(controller) = self.0.take() {
                controller.abort();
            }
        }
    }

    let mut abort = AbortOnDrop(Some(controller));
    let timeout_ms = timeout_secs.saturating_mul(1_000).min(u32::MAX as u64) as u32;
    let request = Box::pin(request.send());
    let timeout = Box::pin(gloo_timers::future::TimeoutFuture::new(timeout_ms));
    match select(request, timeout).await {
        Either::Left((result, _)) => {
            abort.0 = None;
            result.map_err(browser_request_error)
        }
        Either::Right(_) => Err(bkmsa_agent::AgentError::Provider {
            status: 0,
            message: format!("浏览器请求在 {timeout_secs} 秒后超时"),
        }),
    }
}

#[cfg(target_arch = "wasm32")]
async fn read_provider_body(
    response: &gloo_net::http::Response,
) -> bkmsa_agent::Result<serde_json::Value> {
    const LIMIT: usize = 8 * 1024 * 1024;
    if response
        .headers()
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|length| length > LIMIT)
    {
        return Err(bkmsa_agent::AgentError::Provider {
            status: response.status(),
            message: "provider response exceeds 8 MiB limit".into(),
        });
    }
    let stream = response
        .body()
        .ok_or_else(|| bkmsa_agent::AgentError::Provider {
            status: response.status(),
            message: "provider response has no body".into(),
        })?;
    let reader: web_sys::ReadableStreamDefaultReader = stream
        .get_reader()
        .dyn_into()
        .map_err(|value: js_sys::Object| browser_js_error(value.into()))?;
    let mut bytes = Vec::new();
    loop {
        let item = wasm_bindgen_futures::JsFuture::from(reader.read())
            .await
            .map_err(browser_js_error)?;
        if js_sys::Reflect::get(&item, &JsValue::from_str("done"))
            .map_err(browser_js_error)?
            .as_bool()
            .unwrap_or(false)
        {
            break;
        }
        let value =
            js_sys::Reflect::get(&item, &JsValue::from_str("value")).map_err(browser_js_error)?;
        let chunk = js_sys::Uint8Array::new(&value);
        if bytes.len().saturating_add(chunk.length() as usize) > LIMIT {
            let _ = reader.cancel();
            return Err(bkmsa_agent::AgentError::Provider {
                status: response.status(),
                message: "provider response exceeds 8 MiB limit".into(),
            });
        }
        let start = bytes.len();
        bytes.resize(start + chunk.length() as usize, 0);
        chunk.copy_to(&mut bytes[start..]);
    }
    serde_json::from_slice(&bytes).map_err(Into::into)
}

#[cfg(target_arch = "wasm32")]
fn browser_response_text(body: &serde_json::Value) -> Option<String> {
    let message = body.pointer("/choices/0/message")?;
    let content = match message.get("content") {
        Some(serde_json::Value::String(text)) => Some(text.clone()),
        Some(serde_json::Value::Array(parts)) => {
            let text = parts
                .iter()
                .filter(|part| {
                    part.get("type")
                        .and_then(serde_json::Value::as_str)
                        .is_none_or(|kind| kind == "text")
                })
                .filter_map(|part| part.get("text").and_then(serde_json::Value::as_str))
                .collect::<String>();
            (!text.is_empty()).then_some(text)
        }
        _ => None,
    };
    content
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
}

#[cfg(target_arch = "wasm32")]
fn supports_temperature(model: &str) -> bool {
    let model = model.trim().to_ascii_lowercase();
    !["gpt-5", "o1", "o3", "o4"]
        .iter()
        .any(|prefix| model == *prefix || model.starts_with(&format!("{prefix}-")))
}

#[cfg(target_arch = "wasm32")]
fn browser_request_error(error: gloo_net::Error) -> bkmsa_agent::AgentError {
    bkmsa_agent::AgentError::Provider {
        status: 0,
        message: format!("浏览器请求失败，可能被 CORS 阻止: {error}"),
    }
}

#[cfg(target_arch = "wasm32")]
fn browser_js_error(error: JsValue) -> bkmsa_agent::AgentError {
    bkmsa_agent::AgentError::Provider {
        status: 0,
        message: error
            .as_string()
            .unwrap_or_else(|| "browser API error".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_reports_round_trip_through_the_wasm_boundary() {
        let report_json = parse_text("TPS: 19.8", "clipboard").expect("parse text");
        let report: Report = serde_json::from_str(&report_json).expect("deserialize report");
        assert_eq!(report.source, "clipboard");

        let overview =
            execute_report_tool(&report_json, "overview", "{}").expect("execute overview");
        assert!(overview.contains("clipboard"));
    }
}
