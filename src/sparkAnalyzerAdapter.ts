import type {
  AiChatRequest,
  AiConfig,
  AiModelInfo,
  RemoteReport,
  SavePathOptions,
  SparkAnalyzerAdapter,
} from "../packages/spark-analyzer/src";

type JsonRecord = Record<string, unknown>;

export const isTauriRuntime = () => "__TAURI_INTERNALS__" in window;

export const sparkAnalyzerAdapter: SparkAnalyzerAdapter = {
  async fetchReportFromUrl(input: string) {
    if (isTauriRuntime()) {
      const { invoke } = await import("@tauri-apps/api/core");
      return invoke<RemoteReport>("fetch_report_from_url", { input });
    }
    return fetchReportInBrowser(input);
  },

  async pickSavePath(options: SavePathOptions) {
    if (isTauriRuntime()) {
      const { save } = await import("@tauri-apps/plugin-dialog");
      return save(options);
    }
    return options.defaultPath;
  },

  async saveExportFile(path: string, bytesBase64: string) {
    if (isTauriRuntime()) {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("save_export_file", {
        request: {
          path,
          bytes_base64: bytesBase64,
        },
      });
      return;
    }
    downloadBase64(path, bytesBase64);
  },

  async openUrl(url: string) {
    const safeUrl = requireHttpUrl(url, "打开链接");
    if (isTauriRuntime()) {
      const { openUrl } = await import("@tauri-apps/plugin-opener");
      await openUrl(safeUrl);
      return;
    }
    const opened = window.open(safeUrl, "_blank", "noopener,noreferrer");
    if (!opened) throw new Error("浏览器阻止了新窗口，请允许本站打开弹窗后重试");
  },

  async callAiChat(request: AiChatRequest) {
    if (isTauriRuntime()) {
      const { invoke } = await import("@tauri-apps/api/core");
      return invoke<string>("call_ai_chat", { request });
    }
    return postAiChat(request, undefined);
  },

  async testAiConnection(config: AiConfig) {
    if (isTauriRuntime()) {
      const { invoke } = await import("@tauri-apps/api/core");
      return invoke<string>("test_ai_connection", { config });
    }
    return postAiChat({
      config,
      messages: [
        { role: "system", content: "You are a connectivity probe. Reply with exactly: OK" },
        { role: "user", content: "ping" },
      ],
    }, 64, true);
  },

  async listAiModels(config: AiConfig) {
    if (isTauriRuntime()) {
      const { invoke } = await import("@tauri-apps/api/core");
      return invoke<AiModelInfo[]>("list_ai_models", { config });
    }
    validateAiConfig(config, false);
    const response = await browserFetch(`${normalizeBaseUrl(config.base_url)}/models`, {
      headers: authHeaders(config.api_key),
    }, "获取模型列表");
    const value = await readJson(response, "模型列表");
    if (!response.ok) throw new Error(`模型列表返回 HTTP ${response.status}: ${JSON.stringify(value)}`);
    const data = Array.isArray(value.data) ? value.data : [];
    const ids = data
      .map((item) => isRecord(item) ? item.id ?? item.name : undefined)
      .filter((id): id is string => typeof id === "string" && Boolean(id.trim()));
    return [...new Set(ids)].sort().map((id) => ({ id }));
  },
};

async function fetchReportInBrowser(input: string): Promise<RemoteReport> {
  const resolvedUrl = resolveSparkReportUrl(input);
  const configuredProxy = import.meta.env.VITE_SPARK_PROXY_URL?.trim();
  const requestUrl = configuredProxy
    ? `${configuredProxy}${configuredProxy.includes("?") ? "&" : "?"}url=${encodeURIComponent(resolvedUrl)}`
    : resolvedUrl;
  const response = await browserFetch(requestUrl, {
    headers: { Accept: "application/x-spark-sampler, application/x-spark-health, application/x-spark-heap, application/octet-stream, */*" },
  }, "拉取远程报告");
  if (!response.ok) throw new Error(`远程服务返回 HTTP ${response.status}`);
  const bytes = new Uint8Array(await response.arrayBuffer());
  return {
    bytes_base64: bytesToBase64(bytes),
    content_type: response.headers.get("content-type") ?? "application/octet-stream",
    resolved_url: resolvedUrl,
  };
}

async function postAiChat(request: AiChatRequest, maxTokens?: number, allowMissingContent = false) {
  validateAiConfig(request.config, true);
  const body: JsonRecord = {
    model: request.config.model.trim(),
    temperature: request.config.temperature,
    messages: request.messages,
  };
  if (maxTokens !== undefined) body.max_tokens = maxTokens;
  const response = await browserFetch(`${normalizeBaseUrl(request.config.base_url)}/chat/completions`, {
    method: "POST",
    headers: {
      ...authHeaders(request.config.api_key),
      "Content-Type": "application/json",
    },
    body: JSON.stringify(body),
  }, "AI 请求");
  const value = await readJson(response, "AI 响应");
  if (!response.ok) throw new Error(`AI 返回 HTTP ${response.status}: ${JSON.stringify(value)}`);
  const choices = Array.isArray(value.choices) ? value.choices : [];
  const first = choices[0];
  const content = isRecord(first) && isRecord(first.message) ? first.message.content : undefined;
  if (typeof content !== "string" || !content.trim()) {
    if (allowMissingContent) return `OK (HTTP ${response.status})`;
    throw new Error(`AI 响应中没有 message.content: ${JSON.stringify(value)}`);
  }
  return content.trim();
}

async function browserFetch(url: string, init: RequestInit, action: string) {
  try {
    return await fetch(url, init);
  } catch (error) {
    const detail = error instanceof Error ? error.message : String(error);
    throw new Error(`${action}失败：浏览器可能被目标服务的 CORS 策略阻止。报告可改用本地文件或配置代理；AI 服务需允许浏览器跨域访问。${detail ? ` (${detail})` : ""}`);
  }
}

async function readJson(response: Response, label: string): Promise<JsonRecord> {
  try {
    const value: unknown = await response.json();
    if (!isRecord(value)) throw new Error("响应不是 JSON 对象");
    return value;
  } catch (error) {
    throw new Error(`解析${label}失败 (HTTP ${response.status}): ${String(error)}`);
  }
}

function validateAiConfig(config: AiConfig, requireModel: boolean) {
  if (!config.base_url.trim()) throw new Error("Base URL 不能为空");
  if (!config.api_key.trim()) throw new Error("API Key 不能为空");
  if (requireModel && !config.model.trim()) throw new Error("Model 不能为空");
}

function normalizeBaseUrl(value: string) {
  const trimmed = value.trim();
  let url: URL;
  try {
    url = new URL(trimmed);
  } catch (error) {
    throw new Error(`AI Base URL 格式无效: ${String(error)}`);
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") {
    throw new Error("AI Base URL 仅支持 HTTP(S)");
  }
  const isLoopback = url.hostname === "localhost"
    || url.hostname === "[::1]"
    || /^127(?:\.\d{1,3}){3}$/.test(url.hostname);
  if (url.protocol === "http:" && !isLoopback) {
    throw new Error("浏览器中的远程 AI Base URL 必须使用 HTTPS，只有本机回环地址可使用 HTTP");
  }
  return url.toString().replace(/\/+$/, "");
}

function requireHttpUrl(value: string, label: string) {
  const url = new URL(value);
  if (url.protocol !== "http:" && url.protocol !== "https:") {
    throw new Error(`${label}仅支持 HTTP(S) URL`);
  }
  return url.toString();
}

function authHeaders(apiKey: string) {
  return { Authorization: `Bearer ${apiKey.trim()}` };
}

function resolveSparkReportUrl(input: string) {
  const trimmed = input.trim();
  if (!trimmed) throw new Error("链接或 key 不能为空");
  let url: URL;
  try {
    url = new URL(trimmed);
  } catch (error) {
    if (trimmed.includes("://")) {
      throw new Error(`无法解析报告链接，请确认格式正确: ${String(error)}`);
    }
    return `https://spark-usercontent.lucko.me/${encodeURIComponent(trimmed.replace(/^\/+|\/+$/g, ""))}`;
  }
  if (url.hostname === "spark-usercontent.lucko.me" || url.hostname.endsWith(".spark-usercontent.lucko.me")) {
    return url.toString();
  }
  if (url.hostname === "spark.lucko.me" || url.hostname.endsWith(".spark.lucko.me")) {
    const segments = url.pathname.split("/").filter((part) => part && part !== "viewer" && part !== "profile");
    const key = segments.at(-1) ?? url.searchParams.get("id") ?? url.searchParams.get("key");
    if (!key) throw new Error("无法从 spark viewer 链接解析报告 key");
    return `https://spark-usercontent.lucko.me/${encodeURIComponent(key)}`;
  }
  return url.toString();
}

function downloadBase64(path: string, value: string) {
  const bytes = base64ToBytes(value);
  const extension = path.split(".").at(-1)?.toLowerCase();
  const mime = extension === "md" ? "text/markdown;charset=utf-8" : extension === "png" ? "image/png" : "application/octet-stream";
  const url = URL.createObjectURL(new Blob([bytes], { type: mime }));
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = path.split(/[\\/]/).at(-1) || "spark-analysis";
  anchor.style.display = "none";
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 1_000);
}

function bytesToBase64(bytes: Uint8Array) {
  let binary = "";
  const chunkSize = 0x8000;
  for (let offset = 0; offset < bytes.length; offset += chunkSize) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + chunkSize));
  }
  return btoa(binary);
}

function base64ToBytes(value: string) {
  const binary = atob(value);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
  return bytes;
}

function isRecord(value: unknown): value is JsonRecord {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}
