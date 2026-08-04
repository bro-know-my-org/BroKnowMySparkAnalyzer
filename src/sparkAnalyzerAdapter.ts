import type {
  AgentTrace,
  AiConfig,
  AiModelInfo,
  AnalysisResult,
  FollowUpMessage,
  JsonRecord,
  LoadedReport,
  SavePathOptions,
  SparkAnalyzerAdapter,
} from "../packages/spark-analyzer/src";

type WasmAnalyzer = {
  loadReportBytes(bytes: Uint8Array, source: string, hint?: string): Promise<LoadedReport> | LoadedReport;
  loadTextReport(text: string, source?: string): Promise<LoadedReport> | LoadedReport;
  executeTool(reportId: string, tool: string, args?: JsonRecord): Promise<unknown> | unknown;
  runAnalysis(reportId: string, config: AiConfig): Promise<AnalysisResult>;
  cancelAnalysis?(reportId: string): Promise<boolean> | boolean;
  askFollowUp(reportId: string, config: AiConfig, traces: AgentTrace[], diagnosis: string, history: FollowUpMessage[], question: string): Promise<string>;
  testAiConnection(config: AiConfig): Promise<string>;
  listAiModels(config: AiConfig): Promise<AiModelInfo[]>;
  loadApiKey?(): Promise<string | null> | string | null;
  storeApiKey?(apiKey: string): Promise<void> | void;
  deleteApiKey?(): Promise<void> | void;
  releaseReport(reportId: string): Promise<void> | void;
};

const MAX_WEB_REPORT_BYTES = 256 * 1024 * 1024;

async function readBoundedBody(response: Response): Promise<Uint8Array> {
  if (!response.body) throw new Error("远程报告响应没有可读取的正文");
  const reader = response.body.getReader();
  const chunks: Uint8Array[] = [];
  let length = 0;
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      if (length + value.byteLength > MAX_WEB_REPORT_BYTES) {
        await reader.cancel("report size limit exceeded");
        throw new Error("远程报告超过 256 MiB 限制");
      }
      chunks.push(value);
      length += value.byteLength;
    }
  } finally {
    reader.releaseLock();
  }
  const bytes = new Uint8Array(length);
  let offset = 0;
  for (const chunk of chunks) {
    bytes.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return bytes;
}

export const isTauriRuntime = () => "__TAURI_INTERNALS__" in window;

let wasmAnalyzerPromise: Promise<WasmAnalyzer> | undefined;

export const sparkAnalyzerAdapter: SparkAnalyzerAdapter = {
  async loadReportBytes(bytes, source, hint = "") {
    if (bytes.byteLength > MAX_WEB_REPORT_BYTES) throw new Error("报告超过 256 MiB 限制");
    if (isTauriRuntime()) {
      return invoke<LoadedReport>("analyzer_load_report_bytes", {
        request: { bytes_base64: bytesToBase64(bytes), source, hint },
      });
    }
    return (await wasmAnalyzer()).loadReportBytes(bytes, source, hint);
  },

  async loadTextReport(text, source) {
    if (isTauriRuntime()) {
      return invoke<LoadedReport>("analyzer_load_text_report", { request: { text, source } });
    }
    return (await wasmAnalyzer()).loadTextReport(text, source);
  },

  async fetchReport(input) {
    if (isTauriRuntime()) return invoke<LoadedReport>("analyzer_fetch_report", { input });
    const analyzer = await wasmAnalyzer();
    const resolvedUrl = resolveSparkReportUrl(input);
    const configuredProxy = import.meta.env.VITE_SPARK_PROXY_URL?.trim();
    const requestUrl = configuredProxy
      ? `${configuredProxy}${configuredProxy.includes("?") ? "&" : "?"}url=${encodeURIComponent(resolvedUrl)}`
      : resolvedUrl;
    let response: Response;
    try {
      response = await fetch(requestUrl, {
        headers: {
          Accept: "application/x-spark-sampler, application/x-spark-health, application/x-spark-heap, application/octet-stream, */*",
        },
      });
    } catch (error) {
      throw new Error(`无法拉取 spark 报告；请检查网络、CORS 或 VITE_SPARK_PROXY_URL。${String(error)}`);
    }
    if (!response.ok) throw new Error(`远程服务返回 HTTP ${response.status}`);
    const contentLength = Number(response.headers.get("content-length"));
    if (Number.isFinite(contentLength) && contentLength > MAX_WEB_REPORT_BYTES) {
      throw new Error("远程报告超过 256 MiB 限制");
    }
    const bytes = await readBoundedBody(response);
    return analyzer.loadReportBytes(
      bytes,
      resolvedUrl,
      `${response.headers.get("content-type") ?? ""} ${resolvedUrl}`,
    );
  },

  async executeTool(reportId, tool, args = {}) {
    if (isTauriRuntime()) {
      return invoke("analyzer_execute_tool", { request: { report_id: reportId, tool, args } });
    }
    return (await wasmAnalyzer()).executeTool(reportId, tool, args);
  },

  async runAnalysis(reportId, config) {
    if (isTauriRuntime()) {
      return invoke<AnalysisResult>("analyzer_run_analysis", { request: { report_id: reportId, config } });
    }
    return (await wasmAnalyzer()).runAnalysis(reportId, config);
  },

  async cancelAnalysis(reportId) {
    if (isTauriRuntime()) {
      return invoke<boolean>("analyzer_cancel_analysis", { request: { report_id: reportId } });
    }
    const analyzer = await wasmAnalyzer();
    return analyzer.cancelAnalysis ? analyzer.cancelAnalysis(reportId) : false;
  },

  async askFollowUp(reportId, config, traces, diagnosis, history, question) {
    if (isTauriRuntime()) {
      return invoke<string>("analyzer_ask_follow_up", {
        request: { report_id: reportId, config, traces, diagnosis, history, question },
      });
    }
    return (await wasmAnalyzer()).askFollowUp(reportId, config, traces, diagnosis, history, question);
  },

  async testAiConnection(config) {
    if (isTauriRuntime()) return invoke<string>("analyzer_test_ai_connection", { config });
    return (await wasmAnalyzer()).testAiConnection(config);
  },

  async listAiModels(config) {
    if (isTauriRuntime()) return invoke<AiModelInfo[]>("analyzer_list_ai_models", { config });
    return (await wasmAnalyzer()).listAiModels(config);
  },

  async loadApiKey() {
    if (isTauriRuntime()) return invoke<string | null>("analyzer_load_api_key", {});
    return null;
  },

  async storeApiKey(apiKey) {
    if (isTauriRuntime()) await invoke("analyzer_store_api_key", { request: { api_key: apiKey } });
  },

  async deleteApiKey() {
    if (isTauriRuntime()) await invoke("analyzer_delete_api_key", {});
  },

  async releaseReport(reportId) {
    if (isTauriRuntime()) {
      await invoke("analyzer_release_report", { request: { report_id: reportId } });
      return;
    }
    await (await wasmAnalyzer()).releaseReport(reportId);
  },

  async pickSavePath(options: SavePathOptions) {
    if (isTauriRuntime()) {
      const { save } = await import("@tauri-apps/plugin-dialog");
      return save(options);
    }
    return options.defaultPath;
  },

  async saveExportFile(path, bytesBase64) {
    if (isTauriRuntime()) {
      await invoke("save_export_file", { request: { path, bytes_base64: bytesBase64 } });
      return;
    }
    downloadBase64(path, bytesBase64);
  },

  async openUrl(url) {
    const safeUrl = requireHttpUrl(url, "打开链接");
    if (isTauriRuntime()) {
      const { openUrl } = await import("@tauri-apps/plugin-opener");
      await openUrl(safeUrl);
      return;
    }
    const opened = window.open(safeUrl, "_blank", "noopener,noreferrer");
    if (!opened) throw new Error("浏览器阻止了新窗口，请允许本站打开弹窗后重试");
  },
};

async function invoke<T>(command: string, args: Record<string, unknown>): Promise<T> {
  const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
  return tauriInvoke<T>(command, args);
}

async function wasmAnalyzer(): Promise<WasmAnalyzer> {
  if (wasmAnalyzerPromise) return wasmAnalyzerPromise;
  const pending = (async () => {
    const modulePath = import.meta.env.VITE_BKMSA_WASM_MODULE?.trim()
      || new URL(/* @vite-ignore */ "../bkmsa-wasm/bkmsa_wasm.js", import.meta.url).href;
    let imported: unknown;
    try {
      imported = await import(/* @vite-ignore */ modulePath) as unknown;
    } catch (error) {
      throw new Error(`Web 分析后端未安装。请构建 bkmsa-wasm 并设置 VITE_BKMSA_WASM_MODULE。${String(error)}`);
    }
    if (!imported || typeof imported !== "object") throw new Error("bkmsa-wasm 模块导出格式无效");
    const module = imported as Record<string, unknown>;
    if (typeof module.default === "function") await (module.default as () => unknown)();
    const AnalyzerConstructor = module.Analyzer;
    const analyzer = module.analyzer
      ?? module.defaultAnalyzer
      ?? (typeof AnalyzerConstructor === "function"
        ? new (AnalyzerConstructor as new () => unknown)()
        : module);
    if (!analyzer || typeof analyzer !== "object") throw new Error("bkmsa-wasm analyzer 导出无效");
    const methods = analyzer as Record<string, unknown>;
    const required = ["loadReportBytes", "loadTextReport", "executeTool", "runAnalysis", "askFollowUp", "testAiConnection", "listAiModels", "releaseReport"];
    const missing = required.filter((name) => typeof methods[name] !== "function");
    if (missing.length) throw new Error(`bkmsa-wasm 适配接口不完整：缺少 ${missing.join(", ")}`);
    return analyzer as WasmAnalyzer;
  })();
  wasmAnalyzerPromise = pending;
  try {
    return await pending;
  } catch (error) {
    if (wasmAnalyzerPromise === pending) wasmAnalyzerPromise = undefined;
    throw error;
  }
}

function resolveSparkReportUrl(input: string) {
  const trimmed = input.trim();
  if (!trimmed) throw new Error("链接或 key 不能为空");
  let url: URL;
  try {
    url = new URL(trimmed);
  } catch {
    if (trimmed.includes("://")) throw new Error("无法解析报告链接，请确认格式正确");
    return `https://spark-usercontent.lucko.me/${encodeURIComponent(trimmed.replace(/^\/+|\/+$/g, ""))}`;
  }
  if (url.hostname === "spark.lucko.me") {
    const parts = url.pathname.split("/").filter((part) => part && part !== "viewer" && part !== "profile");
    const key = parts.at(-1) ?? url.searchParams.get("id") ?? url.searchParams.get("key");
    if (!key) throw new Error("无法从 spark viewer 链接解析报告 key");
    return `https://spark-usercontent.lucko.me/${encodeURIComponent(key)}`;
  }
  if (url.hostname !== "spark-usercontent.lucko.me") {
    throw new Error(`不支持的报告主机：${url.hostname}`);
  }
  if (url.port && url.port !== "443") throw new Error("spark 报告 URL 必须使用默认 HTTPS 端口");
  if (url.username || url.password) throw new Error("报告链接不能包含用户名或密码");
  url.protocol = "https:";
  return url.toString();
}

function requireHttpUrl(value: string, label: string) {
  const url = new URL(value);
  if (url.protocol !== "http:" && url.protocol !== "https:") throw new Error(`${label}仅支持 HTTP(S) URL`);
  return url.toString();
}

function downloadBase64(path: string, value: string) {
  const binary = atob(value);
  const bytes = Uint8Array.from(binary, (character) => character.charCodeAt(0));
  const extension = path.split(".").at(-1)?.toLowerCase();
  const mime = extension === "md" ? "text/markdown;charset=utf-8" : extension === "png" ? "image/png" : "application/octet-stream";
  const url = URL.createObjectURL(new Blob([bytes], { type: mime }));
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = path.split(/[\\/]/).at(-1) || "spark-analysis";
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 1_000);
}

function bytesToBase64(bytes: Uint8Array) {
  let binary = "";
  for (let offset = 0; offset < bytes.length; offset += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x8000));
  }
  return btoa(binary);
}
