import type { AiConfig, AiModelInfo } from "./ai";

export type RemoteReport = {
  bytes_base64: string;
  content_type: string;
  resolved_url: string;
};

export type SavePathOptions = {
  defaultPath: string;
  filters?: Array<{
    name: string;
    extensions: string[];
  }>;
};

export type AiMessage = {
  role: "system" | "user" | "assistant";
  content: string;
};

export type AiChatRequest = {
  config: AiConfig;
  messages: AiMessage[];
};

export interface SparkAnalyzerAdapter {
  fetchReportFromUrl(input: string): Promise<RemoteReport>;
  pickSavePath(options: SavePathOptions): Promise<string | null>;
  saveExportFile(path: string, bytesBase64: string): Promise<void>;
  openUrl(url: string): Promise<void>;
  callAiChat(request: AiChatRequest): Promise<string>;
  testAiConnection(config: AiConfig): Promise<string>;
  listAiModels(config: AiConfig): Promise<AiModelInfo[]>;
}
