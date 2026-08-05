# bkmsa-tauri

`bkmsa-tauri` is the reusable native Tauri 2 backend for BroKnowMySparkAnalyzer. It owns report sessions, deterministic tools, AI analysis, cancellation, remote report fetching, credential storage, and export writes.

```rust
tauri::Builder::default()
    .plugin(bkmsa_tauri::init())
```

Grant `"bkmsa-tauri:default"` in the application capability. Vue hosts can use `createTauriSparkAnalyzerAdapter()` from `@bro-know-my/spark-analyzer/tauri` instead of writing IPC bindings manually.
