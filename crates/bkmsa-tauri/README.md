# bkmsa-tauri

`bkmsa-tauri` is the reusable native Tauri 2 backend for BroKnowMySparkAnalyzer. It owns report sessions, deterministic tools, AI analysis, cancellation, remote report fetching, credential storage, and export writes.

```rust
tauri::Builder::default()
    .plugin(tauri_plugin_dialog::init())
    .plugin(bkmsa_tauri::init())
```

The dialog plugin is required because export writes use a native save dialog and never trust a WebView-provided filesystem path. Grant `"bkmsa-tauri:default"` in the application capability. Vue hosts can use `createTauriSparkAnalyzerAdapter()` from `@bro-know-my/spark-analyzer/tauri` instead of writing IPC bindings manually.

## Host authorization

`init()` preserves standalone behavior: it allows all host capabilities. It is not an application permission prompt. Embedded applications with user grants must use `init_with_authorizer` and implement `HostAuthorizer` in Rust. The callback checks current host policy before network, credential, or export-write side effects; returning `Err` denies the operation.

For example, a host that only permits local report inspection can deny every protected capability:

```rust
struct LocalOnly;

impl bkmsa_tauri::HostAuthorizer for LocalOnly {
    fn authorize(&self, capability: bkmsa_tauri::HostCapability) -> Result<(), String> {
        Err(format!("Host denied {capability:?}"))
    }
}

tauri::Builder::default()
    .plugin(tauri_plugin_dialog::init())
    .plugin(bkmsa_tauri::init_with_authorizer(LocalOnly));
```

The Tauri capability and native save dialog remain required; the authorizer does not replace either. This policy covers this backend's commands, not arbitrary custom frontend adapters or the separate opener plugin. Hosts must configure those capabilities themselves.
