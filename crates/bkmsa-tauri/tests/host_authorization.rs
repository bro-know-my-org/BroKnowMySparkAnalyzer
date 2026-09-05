use bkmsa_tauri::{HostAuthorizer, HostCapability};
use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};

struct DenyNetwork;

impl HostAuthorizer for DenyNetwork {
    fn authorize(&self, capability: HostCapability) -> Result<(), String> {
        match capability {
            HostCapability::Network => Err("host denied network:spark".to_string()),
            _ => Ok(()),
        }
    }
}

struct DenyCredentials;

impl HostAuthorizer for DenyCredentials {
    fn authorize(&self, capability: HostCapability) -> Result<(), String> {
        match capability {
            HostCapability::Credentials => Err("host denied credentials:ai".to_string()),
            _ => Ok(()),
        }
    }
}

fn allow_plugin_command(context: &mut tauri::Context<tauri::test::MockRuntime>, command: &str) {
    context.runtime_authority_mut().__allow_command(
        format!("plugin:bkmsa|{command}"),
        tauri::utils::acl::ExecutionContext::Local,
    );
}

fn invoke_denied(
    authorizer: impl HostAuthorizer + 'static,
    command: &str,
    body: serde_json::Value,
) -> String {
    let mut context = mock_context(noop_assets());
    allow_plugin_command(&mut context, command);
    let app = mock_builder()
        .plugin(bkmsa_tauri::init_with_authorizer(authorizer))
        .build(context)
        .expect("test app should build");
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("test webview should build");

    get_ipc_response(
        &webview,
        tauri::webview::InvokeRequest {
            cmd: format!("plugin:bkmsa|{command}"),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: "tauri://localhost".parse().expect("URL should parse"),
            body: body.into(),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .expect_err("host authorization should reject the operation")
    .as_str()
    .expect("plugin errors should be strings")
    .to_string()
}

#[test]
fn host_can_block_remote_report_network_access_before_the_request() {
    let mut context = mock_context(noop_assets());
    allow_plugin_command(&mut context, "analyzer_fetch_report");
    let app = mock_builder()
        .plugin(bkmsa_tauri::init_with_authorizer(DenyNetwork))
        .build(context)
        .expect("test app should build");
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("test webview should build");

    let response = get_ipc_response(
        &webview,
        tauri::webview::InvokeRequest {
            cmd: "plugin:bkmsa|analyzer_fetch_report".into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: "tauri://localhost".parse().expect("URL should parse"),
            body: serde_json::json!({ "input": "definitely-not-requested" }).into(),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .expect_err("host authorization should reject network access")
    .as_str()
    .expect("plugin errors should be strings")
    .to_string();

    assert_eq!(response, "host denied network:spark");
}

#[test]
fn host_can_block_api_key_storage_before_keyring_access() {
    let mut context = mock_context(noop_assets());
    allow_plugin_command(&mut context, "analyzer_store_api_key");
    let app = mock_builder()
        .plugin(bkmsa_tauri::init_with_authorizer(DenyCredentials))
        .build(context)
        .expect("test app should build");
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("test webview should build");

    let response = get_ipc_response(
        &webview,
        tauri::webview::InvokeRequest {
            cmd: "plugin:bkmsa|analyzer_store_api_key".into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: "tauri://localhost".parse().expect("URL should parse"),
            body: serde_json::json!({
                "request": {
                    "api_key": "must-not-reach-keyring",
                    "base_url": "https://api.openai.com/v1"
                }
            })
            .into(),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .expect_err("host authorization should reject credential access")
    .as_str()
    .expect("plugin errors should be strings")
    .to_string();

    assert_eq!(response, "host denied credentials:ai");
}

#[test]
fn host_can_block_api_key_loading_before_keyring_access() {
    let mut context = mock_context(noop_assets());
    allow_plugin_command(&mut context, "analyzer_load_api_key");
    let app = mock_builder()
        .plugin(bkmsa_tauri::init_with_authorizer(DenyCredentials))
        .build(context)
        .expect("test app should build");
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("test webview should build");

    let response = get_ipc_response(
        &webview,
        tauri::webview::InvokeRequest {
            cmd: "plugin:bkmsa|analyzer_load_api_key".into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: "tauri://localhost".parse().expect("URL should parse"),
            body: serde_json::json!({ "baseUrl": "https://api.openai.com/v1" }).into(),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    )
    .expect_err("host authorization should reject credential access")
    .as_str()
    .expect("plugin errors should be strings")
    .to_string();

    assert_eq!(response, "host denied credentials:ai");
}

#[test]
fn host_can_block_api_key_deletion_before_keyring_access() {
    let response = invoke_denied(
        DenyCredentials,
        "analyzer_delete_api_key",
        serde_json::json!({}),
    );

    assert_eq!(response, "host denied credentials:ai");
}

#[test]
fn host_can_block_ai_network_access_before_provider_access() {
    let response = invoke_denied(
        DenyNetwork,
        "analyzer_test_ai_connection",
        serde_json::json!({
            "config": {
                "base_url": "https://api.openai.com/v1",
                "api_key": "must-not-be-used",
                "model": "gpt-4.1-mini",
                "temperature": 0.2
            }
        }),
    );

    assert_eq!(response, "host denied network:spark");
}

#[test]
fn host_can_block_ai_credential_access_before_provider_access() {
    let response = invoke_denied(
        DenyCredentials,
        "analyzer_test_ai_connection",
        serde_json::json!({
            "config": {
                "base_url": "https://api.openai.com/v1",
                "api_key": "must-not-be-used",
                "model": "gpt-4.1-mini",
                "temperature": 0.2
            }
        }),
    );

    assert_eq!(response, "host denied credentials:ai");
}

#[test]
fn all_ai_commands_check_both_capabilities_before_report_or_provider_access() {
    let config = serde_json::json!({
        "base_url": "https://api.openai.com/v1",
        "api_key": "must-not-be-used",
        "model": "gpt-4.1-mini",
        "temperature": 0.2
    });
    let cases = [
        (
            "analyzer_list_ai_models",
            serde_json::json!({ "config": config }),
        ),
        (
            "analyzer_run_analysis",
            serde_json::json!({ "request": { "report_id": "nonexistent", "config": config } }),
        ),
        (
            "analyzer_ask_follow_up",
            serde_json::json!({ "request": {
                "report_id": "nonexistent",
                "config": config,
                "traces": [],
                "diagnosis": "",
                "history": [],
                "question": "must-not-be-sent"
            } }),
        ),
    ];
    for (command, body) in cases {
        assert_eq!(
            invoke_denied(DenyNetwork, command, body.clone()),
            "host denied network:spark",
            "{command} must reject network access before any side effect or report lookup"
        );
        assert_eq!(
            invoke_denied(DenyCredentials, command, body),
            "host denied credentials:ai",
            "{command} must reject credential access before any side effect or report lookup"
        );
    }
}

#[test]
fn host_can_block_export_before_the_save_dialog() {
    struct DenyFilesystemWrite;

    impl HostAuthorizer for DenyFilesystemWrite {
        fn authorize(&self, capability: HostCapability) -> Result<(), String> {
            match capability {
                HostCapability::FilesystemWrite => Err("host denied filesystem:write".to_string()),
                _ => Ok(()),
            }
        }
    }

    let response = invoke_denied(
        DenyFilesystemWrite,
        "save_export_file",
        serde_json::json!({
            "request": {
                "path": "{}",
                "bytes_base64": ""
            }
        }),
    );

    assert_eq!(response, "host denied filesystem:write");
}
