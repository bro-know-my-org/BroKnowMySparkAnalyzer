export type JsonValue = string | number | boolean | null | JsonValue[] | { [key: string]: JsonValue };
export type JsonRecord = { [key: string]: JsonValue };

export type ReportKind = "sampler" | "health" | "heap" | "text";

export type Finding = {
  severity: "critical" | "warning" | "info";
  title: string;
  detail: string;
};

export type ReportSummary = {
  title: string;
  platform?: string;
  generatedAt?: string;
  durationSeconds?: number;
  tps1m?: number;
  tps5m?: number;
  tps15m?: number;
  msptMedian?: number;
  msptP95?: number;
  msptMax?: number;
  processCpu1m?: number;
  systemCpu1m?: number;
  heapUsedBytes?: number;
  heapMaxBytes?: number;
  entityCount?: number;
  playerCount?: number;
  gc?: string[];
  worlds?: string[];
  topEntities?: Array<{ name: string; value: number }>;
  topHeap?: Array<{ type: string; instances: number; bytes: number }>;
  topHotspots?: Array<{ label: string; samples: number; percent: number; thread: string; source?: string }>;
  findings: Finding[];
};

/** The UI owns only this opaque backend handle and its presentation summary. */
export type LoadedReport = {
  reportId: string;
  kind: ReportKind;
  source: string;
  summary: ReportSummary;
};

export type AiConfig = {
  base_url: string;
  api_key: string;
  model: string;
  temperature: number;
  timeout_secs?: number;
};

export type AiModelInfo = { id: string };

export type AgentTrace = {
  round: number;
  role: "assistant" | "tool" | "system";
  title: string;
  content: string;
};

export type FollowUpMessage = { role: "user" | "assistant"; content: string };

export type AnalysisResult = {
  diagnosis: string;
  traces: AgentTrace[];
  used_tools?: string[];
  rounds?: number;
  reached_round_limit?: boolean;
};

export type SavePathOptions = {
  defaultPath: string;
  filters?: Array<{ name: string; extensions: string[] }>;
};

export interface SparkAnalyzerAdapter {
  loadReportBytes(bytes: Uint8Array, source: string, hint?: string): Promise<LoadedReport>;
  loadTextReport(text: string, source?: string): Promise<LoadedReport>;
  fetchReport(input: string): Promise<LoadedReport>;
  executeTool(reportId: string, tool: string, args?: JsonRecord): Promise<unknown>;
  runAnalysis(reportId: string, config: AiConfig): Promise<AnalysisResult>;
  cancelAnalysis(reportId: string): Promise<boolean>;
  askFollowUp(
    reportId: string,
    config: AiConfig,
    traces: AgentTrace[],
    diagnosis: string,
    history: FollowUpMessage[],
    question: string,
  ): Promise<string>;
  testAiConnection(config: AiConfig): Promise<string>;
  listAiModels(config: AiConfig): Promise<AiModelInfo[]>;
  loadApiKey(): Promise<string | null>;
  storeApiKey(apiKey: string): Promise<void>;
  deleteApiKey(): Promise<void>;
  releaseReport(reportId: string): Promise<void>;
  pickSavePath(options: SavePathOptions): Promise<string | null>;
  saveExportFile(path: string, bytesBase64: string): Promise<void>;
  openUrl(url: string): Promise<void>;
}
