import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import type {
  AiChatRequest,
  RemoteReport,
  SavePathOptions,
  SparkAnalyzerAdapter,
} from "../packages/spark-analyzer/src";
import type { AiConfig, AiModelInfo } from "../packages/spark-analyzer/src";

export const sparkAnalyzerAdapter: SparkAnalyzerAdapter = {
  fetchReportFromUrl(input: string) {
    return invoke<RemoteReport>("fetch_report_from_url", { input });
  },

  pickSavePath(options: SavePathOptions) {
    return save(options);
  },

  saveExportFile(path: string, bytesBase64: string) {
    return invoke("save_export_file", {
      request: {
        path,
        bytes_base64: bytesBase64,
      },
    });
  },

  openUrl(url: string) {
    return openUrl(url);
  },

  callAiChat(request: AiChatRequest) {
    return invoke<string>("call_ai_chat", { request });
  },

  testAiConnection(config: AiConfig) {
    return invoke<string>("test_ai_connection", { config });
  },

  listAiModels(config: AiConfig) {
    return invoke<AiModelInfo[]>("list_ai_models", { config });
  },
};
