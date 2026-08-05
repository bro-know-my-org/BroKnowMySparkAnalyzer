import type {
  AiModelInfo,
  AnalysisResult,
  LoadedReport,
  SparkAnalyzerAdapter,
} from "./adapter";

const command = (name: string) => `plugin:bkmsa|${name}`;

async function invoke<T>(name: string, args: Record<string, unknown>): Promise<T> {
  const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
  return tauriInvoke<T>(command(name), args);
}

function bytesToBase64(bytes: Uint8Array) {
  let binary = "";
  for (let offset = 0; offset < bytes.length; offset += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x8000));
  }
  return btoa(binary);
}

function requireHttpUrl(value: string) {
  const url = new URL(value);
  if (url.protocol !== "http:" && url.protocol !== "https:") {
    throw new Error("打开链接仅支持 HTTP(S) URL");
  }
  return url.toString();
}

/** Creates the thin frontend adapter for applications that register `bkmsa_tauri::init()`. */
export function createTauriSparkAnalyzerAdapter(): SparkAnalyzerAdapter {
  return {
    loadReportBytes(bytes, source, hint = "") {
      return invoke<LoadedReport>("analyzer_load_report_bytes", {
        request: { bytes_base64: bytesToBase64(bytes), source, hint },
      });
    },

    loadTextReport(text, source) {
      return invoke<LoadedReport>("analyzer_load_text_report", {
        request: { text, source },
      });
    },

    fetchReport(input) {
      return invoke<LoadedReport>("analyzer_fetch_report", { input });
    },

    executeTool(reportId, tool, args = {}) {
      return invoke("analyzer_execute_tool", {
        request: { report_id: reportId, tool, args },
      });
    },

    runAnalysis(reportId, config) {
      return invoke<AnalysisResult>("analyzer_run_analysis", {
        request: { report_id: reportId, config },
      });
    },

    cancelAnalysis(reportId) {
      return invoke<boolean>("analyzer_cancel_analysis", {
        request: { report_id: reportId },
      });
    },

    askFollowUp(reportId, config, traces, diagnosis, history, question) {
      return invoke<string>("analyzer_ask_follow_up", {
        request: { report_id: reportId, config, traces, diagnosis, history, question },
      });
    },

    testAiConnection(config) {
      return invoke<string>("analyzer_test_ai_connection", { config });
    },

    listAiModels(config) {
      return invoke<AiModelInfo[]>("analyzer_list_ai_models", { config });
    },

    loadApiKey() {
      return invoke<string | null>("analyzer_load_api_key", {});
    },

    async storeApiKey(apiKey) {
      await invoke("analyzer_store_api_key", { request: { api_key: apiKey } });
    },

    async deleteApiKey() {
      await invoke("analyzer_delete_api_key", {});
    },

    async releaseReport(reportId) {
      await invoke("analyzer_release_report", { request: { report_id: reportId } });
    },

    async pickSavePath(options) {
      const { save } = await import("@tauri-apps/plugin-dialog");
      return save(options);
    },

    async saveExportFile(path, bytesBase64) {
      await invoke("save_export_file", {
        request: { path, bytes_base64: bytesBase64 },
      });
    },

    async openUrl(url) {
      const { openUrl } = await import("@tauri-apps/plugin-opener");
      await openUrl(requireHttpUrl(url));
    },
  };
}
