const COMMANDS: &[&str] = &[
    "analyzer_load_report_bytes",
    "analyzer_load_text_report",
    "analyzer_fetch_report",
    "analyzer_execute_tool",
    "analyzer_release_report",
    "analyzer_run_analysis",
    "analyzer_cancel_analysis",
    "analyzer_ask_follow_up",
    "analyzer_test_ai_connection",
    "analyzer_list_ai_models",
    "analyzer_store_api_key",
    "analyzer_load_api_key",
    "analyzer_delete_api_key",
    "save_export_file",
];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
