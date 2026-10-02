use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};

fn invoke(
    webview: &tauri::WebviewWindow<tauri::test::MockRuntime>,
    command: &str,
    body: Value,
) -> Result<Value, Value> {
    get_ipc_response(
        webview,
        tauri::webview::InvokeRequest {
            cmd: format!("plugin:bkmsa-tauri|{command}"),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: "tauri://localhost".parse().unwrap(),
            body: body.into(),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .map(|response| response.deserialize().unwrap())
}

#[test]
fn desktop_streams_traces_while_provider_is_pending_and_can_cancel() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://{}/v1", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let (requested_tx, requested_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "provider request did not arrive");
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("mock provider failed: {error}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        let mut chunk = [0; 1024];
        while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            let count = stream.read(&mut chunk).unwrap();
            assert!(count > 0);
            request.extend_from_slice(&chunk[..count]);
        }
        requested_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let _ = write!(
            stream,
            "HTTP/1.1 503 Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
    });
    let (trace_tx, trace_rx) = mpsc::channel();
    let mut context = mock_context(noop_assets());
    for command in [
        "analyzer_load_text_report",
        "analyzer_run_analysis",
        "analyzer_cancel_analysis",
    ] {
        context.runtime_authority_mut().__allow_command(
            format!("plugin:bkmsa-tauri|{command}"),
            tauri::utils::acl::ExecutionContext::Local,
        );
    }
    let app = mock_builder()
        .channel_interceptor(move |_webview, callback, index, body| {
            assert_eq!(callback.0, 7);
            trace_tx
                .send((
                    index,
                    body.clone()
                        .deserialize::<bkmsa_agent::AgentTrace>()
                        .unwrap(),
                ))
                .unwrap();
            true
        })
        .plugin(bkmsa_tauri::init())
        .build(context)
        .unwrap();
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let report = invoke(
        &webview,
        "analyzer_load_text_report",
        json!({ "request": { "text": "Can't keep up!" } }),
    )
    .unwrap();
    // Legacy hosts may omit the progress channel entirely.
    let without_channel = invoke(
        &webview,
        "analyzer_run_analysis",
        json!({
            "request": { "report_id": "missing-report", "config": {
                "base_url": base_url, "api_key": "mock", "model": "mock", "temperature": 0.2,
            } },
        }),
    )
    .unwrap_err();
    assert!(without_channel.as_str().unwrap().contains("报告"));
    let analysis_webview = webview.clone();
    let report_id = report["reportId"].clone();
    let analysis = std::thread::spawn(move || {
        invoke(
            &analysis_webview,
            "analyzer_run_analysis",
            json!({
                "request": { "report_id": report_id, "config": {
                    "base_url": base_url, "api_key": "mock", "model": "mock", "temperature": 0.2,
                } }, "onTrace": "__CHANNEL__:7",
            }),
        )
    });
    requested_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let (index, trace) = trace_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(index, 0);
    assert_eq!(trace.title, "Tool: report_inventory");
    assert!(
        !analysis.is_finished(),
        "trace must arrive before analysis resolves"
    );
    assert_eq!(
        invoke(
            &webview,
            "analyzer_cancel_analysis",
            json!({ "request": { "report_id": report["reportId"] } })
        )
        .unwrap(),
        true
    );
    let error = analysis.join().unwrap().unwrap_err();
    assert!(error.as_str().unwrap().contains("分析已中止"));
    assert!(trace_rx.try_recv().is_err());
    release_tx.send(()).unwrap();
    server.join().unwrap();
}
