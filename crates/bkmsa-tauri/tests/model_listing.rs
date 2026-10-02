use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};

fn invoke_models(config: Value) -> Value {
    let mut context = mock_context(noop_assets());
    context.runtime_authority_mut().__allow_command(
        "plugin:bkmsa-tauri|analyzer_list_ai_models".into(),
        tauri::utils::acl::ExecutionContext::Local,
    );
    let app = mock_builder()
        .plugin(bkmsa_tauri::init())
        .build(context)
        .unwrap();
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    get_ipc_response(
        &webview,
        tauri::webview::InvokeRequest {
            cmd: "plugin:bkmsa-tauri|analyzer_list_ai_models".into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: "tauri://localhost".parse().unwrap(),
            body: json!({ "config": config }).into(),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .expect("model discovery should succeed with an empty model")
    .deserialize()
    .unwrap()
}

#[test]
fn desktop_ipc_lists_models_before_model_selection() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base_url = format!("http://{}/v1/", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "model request did not arrive");
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
        let request = String::from_utf8(request).unwrap().to_ascii_lowercase();
        assert!(request.starts_with("get /v1/models http/1.1\r\n"));
        assert!(request.contains("authorization: bearer test-key\r\n"));
        let body = r#"{"object":"list","data":[{"id":"test-model","owned_by":"mock"}]}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
    });
    let models = invoke_models(json!({
        "base_url": base_url,
        "api_key": "test-key",
        "model": "",
        "temperature": 0.2,
        "timeout_secs": 5
    }));
    server.join().unwrap();
    assert_eq!(models, json!([{ "id": "test-model" }]));
}

#[test]
#[ignore = "requires BKMSA_TEST_BASE_URL and BKMSA_TEST_API_KEY for a live provider"]
fn desktop_ipc_lists_live_models_before_model_selection() {
    let models = invoke_models(json!({
        "base_url": std::env::var("BKMSA_TEST_BASE_URL").unwrap(),
        "api_key": std::env::var("BKMSA_TEST_API_KEY").unwrap(),
        "model": "",
        "temperature": 0.2,
        "timeout_secs": 30
    }));
    let models = models.as_array().unwrap();
    assert!(!models.is_empty());
    assert!(models.iter().all(|model| model["id"].as_str().is_some()));
    println!(
        "Live desktop IPC model listing returned {} models",
        models.len()
    );
}
