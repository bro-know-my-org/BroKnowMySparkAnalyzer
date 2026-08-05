# bkmsa-tauri

`bkmsa-tauri` is the reusable native Tauri 2 backend for BroKnowMySparkAnalyzer. It owns report sessions, deterministic tools, AI analysis, cancellation, remote report fetching, credential storage, and export writes.

```rust
tauri::Builder::default()
    .plugin(tauri_plugin_dialog::init())
    .plugin(bkmsa_tauri::init())
```

The dialog plugin is required because export writes use a native save dialog and never trust a WebView-provided filesystem path. Grant `"bkmsa-tauri:default"` in the application capability. Vue hosts can use `createTauriSparkAnalyzerAdapter()` from `@bro-know-my/spark-analyzer/tauri` instead of writing IPC bindings manually.
