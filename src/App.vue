<script setup lang="ts">
import { computed, h, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Dismiss20Regular, LineHorizontal120Regular, Maximize20Regular } from "@vicons/fluent";
import { Github, Language } from "@vicons/fa";
import { toPng } from "html-to-image";
import { marked } from "marked";
import {
  darkTheme,
  lightTheme,
  NAlert,
  NButton,
  NCollapse,
  NCollapseItem,
  NConfigProvider,
  NDivider,
  NIcon,
  NInput,
  NInputNumber,
  NLayout,
  NLayoutSider,
  NMessageProvider,
  NModal,
  NSelect,
  NSpace,
  NStatistic,
  NSwitch,
  NTag,
  NText,
  NTooltip,
  createDiscreteApi,
  type SelectOption,
} from "naive-ui";
import {
  formatBytes,
  formatNumber,
  parseReportBytes,
  parseTextReport,
  type ReportDocument,
} from "./report";
import {
  askFollowUp,
  listModels,
  providerPresets,
  runToolAgent,
  testConnection,
  type AgentTrace,
  type AiConfig,
  type FollowUpMessage,
} from "./ai";

type RemoteReport = {
  bytes_base64: string;
  content_type: string;
  resolved_url: string;
};

type Lang = "zh" | "en";
type ThemeMode = "dark" | "light";
type StatusKey =
  | "waiting"
  | "parsing"
  | "loaded"
  | "parseFailed"
  | "fetching"
  | "fetchFailed"
  | "textLoaded"
  | "analyzing"
  | "done"
  | "failed";

const copy = {
  zh: {
    status: {
      waiting: "等待报告中",
      parsing: "解析报告中",
      loaded: "报告已载入",
      parseFailed: "解析失败",
      fetching: "拉取远程报告中",
      fetchFailed: "拉取失败",
      textLoaded: "文本已载入",
      analyzing: "Agent 分析中",
      done: "Agent 分析完成",
      failed: "Agent 分析失败",
    },
    ui: {
      subtitle: "Spark Agent Workbench",
      clear: "清空",
      debug: "Debug",
      report: "报告",
      reportTip: "拖入 .sparkprofile 或 .sparkheap；Tauri 已关闭窗口级拖拽捕获。",
      dropTitle: "拖入 spark 报告",
      dropHint: ".sparkprofile / .sparkheap / health protobuf",
      sourcePlaceholder: "Spark 报告链接 / bytebin key",
      fetch: "拉取链接",
      fetching: "拉取中",
      optionalTextInput: "可选：粘贴文本/日志",
      textPlaceholder: "粘贴日志或人工摘要",
      loadText: "载入文本",
      aiTip: "OpenAI-compatible /chat/completions；测试不会保存 key。",
      advancedAi: "高级 AI 设置",
      temperature: "温度",
      temperatureTip: "越低越稳定；性能诊断建议保持 0.2。",
      getModels: "获取模型",
      baseUrlPlaceholder: "Base URL（接口地址）",
      apiKeyPlaceholder: "API Key（密钥）",
      modelPlaceholder: "选择或输入模型",
      test: "测试连通性",
      analyze: "Agent 分析",
      type: "类型",
      heap: "堆内存",
      entities: "实体",
      reportStatus: "报告状态",
      noReport: "尚未载入报告",
      trace: "工具轨迹",
      noTrace: "Agent 尚未调用工具。",
      diagnosis: "诊断结论",
      followUp: "继续追问",
      followUpPlaceholder: "追问这份报告的细节，例如：commands 热点具体是什么？为什么不能锁定坐标？",
      ask: "发送",
      fullscreen: "全屏",
      exportMd: "导出 MD",
      exportImage: "生成图片",
      close: "关闭",
      noDiagnosis: "尚未生成诊断。",
      raw: "原文",
      summary: "摘要",
      round: "轮次",
      aiRequest: "AI 请求",
      aiMessage: "AI 输出",
      toolResult: "工具返回",
      systemNote: "系统提示",
      arguments: "参数",
      language: "语言",
      theme: "主题",
      dark: "深色",
      light: "浅色",
    },
    msg: {
      loaded: "已载入",
      remoteLoaded: "远程报告已载入",
      sourceRequired: "请输入 spark 链接或 key",
      textRequired: "文本不能为空",
      loadReportFirst: "先载入报告",
      connected: "连通性正常",
      modelsLoaded: "模型列表已更新",
      modelFetchFailed: "获取模型失败",
      exported: "已导出",
      exportFailed: "导出失败",
      noDiagnosis: "还没有可导出的诊断结论",
    },
  },
  en: {
    status: {
      waiting: "Waiting for report",
      parsing: "Parsing report",
      loaded: "Report loaded",
      parseFailed: "Parse failed",
      fetching: "Fetching remote report",
      fetchFailed: "Fetch failed",
      textLoaded: "Text loaded",
      analyzing: "Agent analyzing",
      done: "Agent complete",
      failed: "Agent failed",
    },
    ui: {
      subtitle: "Spark Agent Workbench",
      clear: "Clear",
      debug: "Debug",
      report: "Report",
      reportTip: "Drop .sparkprofile or .sparkheap files. Tauri window drag capture is disabled.",
      dropTitle: "Drop spark report",
      dropHint: ".sparkprofile / .sparkheap / health protobuf",
      sourcePlaceholder: "spark viewer URL / bytebin key",
      fetch: "Fetch URL",
      fetching: "Fetching",
      optionalTextInput: "Optional: paste text/logs",
      textPlaceholder: "Paste logs or manual notes",
      loadText: "Load Text",
      aiTip: "OpenAI-compatible /chat/completions. Test does not save the key.",
      advancedAi: "Advanced AI Settings",
      temperature: "Temperature",
      temperatureTip: "Lower is more stable. 0.2 is recommended for diagnostics.",
      getModels: "Get Models",
      baseUrlPlaceholder: "Base URL",
      apiKeyPlaceholder: "API Key",
      modelPlaceholder: "Select or type a model",
      test: "Test Connection",
      analyze: "Agent Analyze",
      type: "Type",
      heap: "Heap",
      entities: "Entities",
      reportStatus: "Report Status",
      noReport: "No report loaded",
      trace: "Tool Trace",
      noTrace: "Agent has not called tools yet.",
      diagnosis: "Diagnosis",
      followUp: "Follow-up",
      followUpPlaceholder: "Ask about this report, for example: what are the command hotspots?",
      ask: "Send",
      fullscreen: "Fullscreen",
      exportMd: "Export MD",
      exportImage: "Export Image",
      close: "Close",
      noDiagnosis: "No diagnosis yet.",
      raw: "Raw",
      summary: "Summary",
      round: "Round",
      aiRequest: "AI Request",
      aiMessage: "AI Output",
      toolResult: "Tool Result",
      systemNote: "System Note",
      arguments: "Args",
      language: "Language",
      theme: "Theme",
      dark: "Dark",
      light: "Light",
    },
    msg: {
      loaded: "Loaded",
      remoteLoaded: "Remote report loaded",
      sourceRequired: "Enter a spark URL or key",
      textRequired: "Text cannot be empty",
      loadReportFirst: "Load a report first",
      connected: "Connection OK",
      modelsLoaded: "Model list updated",
      modelFetchFailed: "Failed to fetch models",
      exported: "Exported",
      exportFailed: "Export failed",
      noDiagnosis: "No diagnosis to export yet",
    },
  },
} satisfies Record<Lang, any>;

const { message } = createDiscreteApi(["message"], {
  configProviderProps: {
    theme: darkTheme,
  },
});
const appWindow = getCurrentWindow();
const report = ref<ReportDocument | null>(null);
const statusKey = ref<StatusKey>("waiting");
const sourceInput = ref("");
const textInput = ref("");
const dragging = ref(false);
const aiOutput = ref("");
const traces = ref<AgentTrace[]>([]);
const busy = ref(false);
const fetchingReport = ref(false);
const testing = ref(false);
const fetchingModels = ref(false);
const followUpBusy = ref(false);
const followUpInput = ref("");
const followUps = ref<FollowUpMessage[]>([]);
const diagnosisFullscreen = ref(false);
const diagnosisRef = ref<HTMLElement | null>(null);
const providerId = ref("custom");
const apiKey = ref("");
const baseUrl = ref(providerPresets[0]?.baseUrl ?? "");
const model = ref(providerPresets[0]?.model ?? "");
const fetchedModels = ref<string[]>([]);
const temperature = ref(0.2);
const language = ref<Lang>("zh");
const themeMode = ref<ThemeMode>("dark");
const debugMode = ref(false);
const altPressed = ref(false);

const t = computed(() => copy[language.value]);
const status = computed(() => t.value.status[statusKey.value]);
const naiveTheme = computed(() => (themeMode.value === "dark" ? darkTheme : lightTheme));
const languageOptions: SelectOption[] = [
  { label: "中文", value: "zh" },
  { label: "English", value: "en" },
];
const lightThemeEnabled = computed({
  get: () => themeMode.value === "light",
  set: (enabled: boolean) => {
    themeMode.value = enabled ? "light" : "dark";
  },
});

function preventNativeContextMenu(event: MouseEvent) {
  event.preventDefault();
}

function updateAltPressed(event: KeyboardEvent) {
  altPressed.value = event.altKey;
}

function releaseAltPressed() {
  altPressed.value = false;
}

onMounted(() => {
  document.addEventListener("contextmenu", preventNativeContextMenu);
  window.addEventListener("keydown", updateAltPressed);
  window.addEventListener("keyup", updateAltPressed);
  window.addEventListener("blur", releaseAltPressed);
});

onBeforeUnmount(() => {
  document.removeEventListener("contextmenu", preventNativeContextMenu);
  window.removeEventListener("keydown", updateAltPressed);
  window.removeEventListener("keyup", updateAltPressed);
  window.removeEventListener("blur", releaseAltPressed);
});

const providerOptions = computed<SelectOption[]>(() =>
  providerPresets
    .filter((preset) => debugMode.value || preset.id !== "newapi-happy")
    .map((preset) => ({
      label: preset.name,
      value: preset.id,
    })),
);

const modelOptions = computed<SelectOption[]>(() => {
  const models = new Set<string>(fetchedModels.value);
  if (model.value.trim()) models.add(model.value.trim());
  return [...models].map((id) => ({ label: id, value: id }));
});

const summary = computed(() => report.value?.summary);
const canAnalyze = computed(() => Boolean(report.value && baseUrl.value && model.value && apiKey.value && !busy.value));
const canAskFollowUp = computed(() => Boolean(report.value && aiOutput.value && followUpInput.value.trim() && !followUpBusy.value && !busy.value));
const renderedMarkdown = computed(() => {
  const source = aiOutput.value || t.value.ui.noDiagnosis;
  return marked.parse(source, { async: false }) as string;
});

function currentConfig(): AiConfig {
  return {
    base_url: baseUrl.value.trim(),
    api_key: apiKey.value.trim(),
    model: model.value.trim(),
    temperature: Number(temperature.value ?? 0.2),
  };
}

function applyProvider(value: string) {
  const preset = providerPresets.find((item) => item.id === value);
  if (!preset) return;
  providerId.value = preset.id;
  baseUrl.value = preset.baseUrl;
  model.value = preset.model;
  fetchedModels.value = [];
}

watch(debugMode, (enabled) => {
  if (!enabled && providerId.value === "newapi-happy") {
    applyProvider("custom");
  }
});

async function handleFiles(files: FileList | File[]) {
  const file = Array.from(files)[0];
  if (!file) return;
  statusKey.value = "parsing";
  try {
    const bytes = new Uint8Array(await file.arrayBuffer());
    report.value = await parseReportBytes(bytes, file.name, file.name);
    traces.value = [];
    aiOutput.value = "";
    followUps.value = [];
    followUpInput.value = "";
    statusKey.value = "loaded";
    message.success(`${t.value.msg.loaded} ${file.name}`);
  } catch (error) {
    statusKey.value = "parseFailed";
    message.error(String(error));
  }
}

async function fetchRemoteReport() {
  if (!sourceInput.value.trim()) {
    message.warning(t.value.msg.sourceRequired);
    return;
  }
  statusKey.value = "fetching";
  fetchingReport.value = true;
  try {
    const remote = await invoke<RemoteReport>("fetch_report_from_url", { input: sourceInput.value.trim() });
    const bytes = base64ToBytes(remote.bytes_base64);
    report.value = await parseReportBytes(bytes, remote.resolved_url, `${remote.content_type} ${remote.resolved_url}`);
    traces.value = [];
    aiOutput.value = "";
    followUps.value = [];
    followUpInput.value = "";
    statusKey.value = "loaded";
    message.success(t.value.msg.remoteLoaded);
  } catch (error) {
    statusKey.value = "fetchFailed";
    message.error(String(error));
  } finally {
    fetchingReport.value = false;
  }
}

function analyzeText() {
  const text = textInput.value.trim();
  if (!text) {
    message.warning(t.value.msg.textRequired);
    return;
  }
  report.value = parseTextReport(text);
  traces.value = [];
  aiOutput.value = "";
  followUps.value = [];
  followUpInput.value = "";
  statusKey.value = "textLoaded";
}

async function testAi() {
  testing.value = true;
  try {
    const result = await testConnection(currentConfig());
    message.success(`${t.value.msg.connected}: ${result.slice(0, 40) || "OK"}`);
  } catch (error) {
    message.error(String(error));
  } finally {
    testing.value = false;
  }
}

async function fetchModels() {
  fetchingModels.value = true;
  try {
    const models = await listModels(currentConfig());
    fetchedModels.value = models.map((item) => item.id).filter(Boolean);
    message.success(`${t.value.msg.modelsLoaded}: ${fetchedModels.value.length}`);
  } catch (error) {
    message.error(`${t.value.msg.modelFetchFailed}: ${String(error)}`);
  } finally {
    fetchingModels.value = false;
  }
}

async function runAnalysis() {
  if (!report.value) {
    message.warning(t.value.msg.loadReportFirst);
    return;
  }
  busy.value = true;
  traces.value = [];
  aiOutput.value = "";
  followUps.value = [];
  followUpInput.value = "";
  statusKey.value = "analyzing";
  try {
    const final = await runToolAgent(report.value, currentConfig(), (trace) => {
      traces.value.push(trace);
    });
    aiOutput.value = final;
    statusKey.value = "done";
  } catch (error) {
    aiOutput.value = `## 分析失败\n\n${String(error)}`;
    statusKey.value = "failed";
  } finally {
    busy.value = false;
  }
}

async function sendFollowUp() {
  const question = followUpInput.value.trim();
  if (!question || !report.value || !aiOutput.value) return;
  followUpInput.value = "";
  followUps.value.push({ role: "user", content: question });
  followUpBusy.value = true;
  try {
    const answer = await askFollowUp(
      report.value,
      currentConfig(),
      traces.value,
      aiOutput.value,
      followUps.value.slice(0, -1),
      question,
    );
    followUps.value.push({ role: "assistant", content: answer });
  } catch (error) {
    followUps.value.push({ role: "assistant", content: `追问失败：${String(error)}` });
  } finally {
    followUpBusy.value = false;
  }
}

function clearAll() {
  report.value = null;
  sourceInput.value = "";
  textInput.value = "";
  traces.value = [];
  aiOutput.value = "";
  followUps.value = [];
  followUpInput.value = "";
  statusKey.value = "waiting";
}

function renderFollowUp(content: string) {
  return marked.parse(content, { async: false }) as string;
}

function diagnosisMarkdown() {
  return aiOutput.value.trim();
}

async function exportMarkdown() {
  const markdown = diagnosisMarkdown();
  if (!markdown) {
    message.warning(t.value.msg.noDiagnosis);
    return;
  }
  const source = report.value?.source ? `\n\n---\nsource: ${report.value.source}\n` : "";
  const path = await save({
    defaultPath: `${exportBaseName()}.md`,
    filters: [{ name: "Markdown", extensions: ["md"] }],
  });
  if (!path) return;
  await saveBytes(path, stringToBase64(`${markdown}${source}`));
  message.success(`${t.value.msg.exported} ${path}`);
}

async function exportDiagnosisImage() {
  if (!diagnosisMarkdown()) {
    message.warning(t.value.msg.noDiagnosis);
    return;
  }
  const exportNode = document.createElement("section");
  exportNode.className = `markdown-body image-export-node ${themeMode.value === "light" ? "image-export-light" : ""}`;
  exportNode.innerHTML = renderedMarkdown.value;
  document.body.appendChild(exportNode);
  try {
    const dataUrl = await toPng(exportNode, {
      cacheBust: true,
      pixelRatio: 2,
      backgroundColor: themeMode.value === "dark" ? "#10161b" : "#f8fafb",
    });
    const path = await save({
      defaultPath: `${exportBaseName()}.png`,
      filters: [{ name: "PNG Image", extensions: ["png"] }],
    });
    if (!path) return;
    await saveBytes(path, dataUrlToBase64(dataUrl));
    message.success(`${t.value.msg.exported} ${path}`);
  } catch (error) {
    message.error(`${t.value.msg.exportFailed}: ${String(error)}`);
  } finally {
    exportNode.remove();
  }
}

async function saveBytes(path: string, bytesBase64: string) {
  await invoke("save_export_file", {
    request: {
      path,
      bytes_base64: bytesBase64,
    },
  });
}

function minimizeWindow() {
  void appWindow.minimize();
}

function toggleMaximizeWindow() {
  void appWindow.toggleMaximize();
}

function closeWindow() {
  void appWindow.close();
}

function openGitHub() {
  void openUrl("https://github.com/bro-know-my-org/BroKnowMySparkAnalyzer");
}

function startWindowDrag(event: MouseEvent) {
  if (event.button !== 0 || event.detail > 1) return;
  void appWindow.startDragging();
}

function stringToBase64(value: string) {
  const bytes = new TextEncoder().encode(value);
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return window.btoa(binary);
}

function dataUrlToBase64(value: string) {
  const index = value.indexOf(",");
  return index >= 0 ? value.slice(index + 1) : value;
}

function exportBaseName() {
  const source = report.value?.source?.split(/[\\/]/).pop()?.replace(/\.[^.]+$/, "") || "spark-diagnosis";
  return source.replace(/[^a-zA-Z0-9._-]+/g, "_").slice(0, 80) || "spark-diagnosis";
}

function onDrop(event: DragEvent) {
  event.preventDefault();
  dragging.value = false;
  if (event.dataTransfer?.files?.length) {
    void handleFiles(event.dataTransfer.files);
  }
}

function base64ToBytes(value: string) {
  const binary = window.atob(value);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return bytes;
}

function severityType(severity: string) {
  if (severity === "critical") return "error";
  if (severity === "warning") return "warning";
  return "info";
}

function traceType(trace: AgentTrace) {
  if (trace.role === "tool") return "info";
  if (trace.role === "system") return "warning";
  return "success";
}

function traceRoleClass(trace: AgentTrace) {
  if (trace.role === "tool") return "tool";
  if (trace.role === "system") return "system";
  return parseToolRequest(trace.content) ? "request" : "ai";
}

function traceTitle(trace: AgentTrace) {
  if (trace.role === "tool") return `${t.value.ui.toolResult}: ${toolDisplayName(traceToolName(trace))}`;
  if (trace.role === "system") return t.value.ui.systemNote;

  const request = parseToolRequest(trace.content);
  if (request) return `${t.value.ui.aiRequest}: ${toolDisplayName(request.tool)}`;

  return t.value.ui.aiMessage;
}

function traceSubhead(trace: AgentTrace) {
  const toolName = traceToolName(trace);
  const request = trace.role === "assistant" ? parseToolRequest(trace.content) : null;
  if (request) return `${request.tool}${formatArgs(request.args)}`;
  if (toolName) return toolName;
  return trace.title;
}

function traceSummary(trace: AgentTrace) {
  const request = parseToolRequest(trace.content);
  if (request) {
    return toolIntent(request.tool, request.args);
  }

  if (trace.role !== "tool") {
    return firstUsefulLine(trace.content) || trace.content.slice(0, 160);
  }

  const parsed = parseJson(trace.content);
  const tool = traceToolName(trace);
  if (!parsed) return firstUsefulLine(trace.content) || "-";

  if (tool === "report_inventory") {
    const data = parsed.availableData ? Object.entries(parsed.availableData).filter(([, value]) => value).length : 0;
    return language.value === "zh"
      ? `${parsed.kind ?? "-"} 报告，可用 ${data} 类数据`
      : `${parsed.kind ?? "-"} report · ${data} data families`;
  }
  if (tool === "overview") {
    const metrics = parsed.metrics ?? {};
    return `TPS 1m ${formatNumber(metrics.tps1m)} · MSPT max ${formatNumber(metrics.msptMax)} · ${t.value.ui.entities} ${formatNumber(metrics.entityCount)}`;
  }
  if (tool === "hotspots") {
    const hotspots = parsed.hotspots ?? parsed;
    return language.value === "zh" ? `最高热点：${hotspots?.[0]?.label ?? "-"}` : `Top hotspot: ${hotspots?.[0]?.label ?? "-"}`;
  }
  if (tool === "hotspot_groups") {
    return `${language.value === "zh" ? "热点类别" : "Categories"}: ${(parsed.byCategory ?? []).slice(0, 4).map((item: any) => `${categoryDisplayName(item.category)} ${formatNumber(item.maxPercent)}%`).join(" · ")}`;
  }
  if (tool === "hot_paths") {
    return `${language.value === "zh" ? "热点子路径" : "Hot paths"}: ${(parsed.frames ?? []).slice(0, 4).map((item: any) => `${item.role ?? "frame"} ${formatNumber(item.maxPercent)}% ${shortClassName(item.className ?? item.label)}`).join(" · ")}`;
  }
  if (tool === "mod_sources") {
    return `${language.value === "zh" ? "模组来源" : "Sources"}: ${prioritizedSources(parsed).slice(0, 4).map((item: any) => `${item.name ?? item.sourceId} ${formatNumber(item.maxPercent)}%`).join(" · ")}`;
  }
  if (tool === "time_windows") {
    return language.value === "zh" ? `读取 ${parsed.windows?.length ?? 0} 个时间窗口` : `${parsed.windows?.length ?? 0} windows`;
  }
  if (tool === "worst_windows") {
    const worst = parsed.worstByMaxMspt?.[0];
    return worst
      ? `${language.value === "zh" ? "最坏窗口" : "Worst"} ${worst.id}: max MSPT ${formatNumber(worst.msptMax)}, TPS ${formatNumber(worst.tps)}`
      : "-";
  }
  if (tool === "entities") {
    return `${t.value.ui.entities} ${formatNumber(parsed.totalEntities)} · ${(parsed.topEntities ?? []).slice(0, 4).map((item: any) => `${item.name}=${item.value}`).join(" · ")}`;
  }
  if (tool === "entity_chunks") {
    return `${language.value === "zh" ? "实体密集区块" : "Top chunks"}: ${(parsed.topChunks ?? []).slice(0, 3).map((chunk: any) => `${chunk.world} ${chunk.x},${chunk.z}=${chunk.totalEntities}`).join(" · ")}`;
  }
  if (tool === "memory_gc") {
    const worstGc = parsed.gcCollectors?.[0];
    const signal = parsed.signals?.[0];
    return worstGc
      ? `${language.value === "zh" ? "GC/内存" : "GC/memory"}: ${worstGc.name} avg ${formatNumber(worstGc.avgTimeMs)}ms · ${signal?.title ?? parsed.interpretation}`
      : parsed.interpretation ?? "-";
  }
  if (tool === "diagnostic_hypotheses") {
    return `${language.value === "zh" ? "候选结论" : "Hypotheses"}: ${(parsed.hypotheses ?? []).slice(0, 4).map((item: any) => `${hypothesisDisplayName(item.id)}(${confidenceDisplayName(item.confidence)})`).join(" · ")}`;
  }
  if (tool === "evidence_gaps") {
    return language.value === "zh"
      ? `可用证据 ${parsed.availableEvidence?.length ?? 0} 类 · 缺失 ${parsed.missingEvidence?.length ?? 0} 类`
      : `Available ${parsed.availableEvidence?.length ?? 0} · missing ${parsed.missingEvidence?.length ?? 0}`;
  }
  return summarizeJson(parsed);
}

function traceToolName(trace: AgentTrace) {
  if (trace.role === "tool") return trace.title.replace(/^Tool:\s*/, "");
  return parseToolRequest(trace.content)?.tool ?? "";
}

function parseToolRequest(value: string): { tool: string; args?: any } | null {
  const trimmed = value.trim();
  const candidates = [
    trimmed,
    trimmed.match(/```(?:json)?\s*([\s\S]*?)```/)?.[1] ?? "",
    trimmed.match(/\{[\s\S]*\}/)?.[0] ?? "",
  ].filter(Boolean);

  for (const candidate of candidates) {
    const parsed = parseJson(candidate);
    if (parsed && typeof parsed.tool === "string") return { tool: parsed.tool, args: parsed.args ?? {} };
  }
  return null;
}

function formatArgs(args: any) {
  if (!args || Object.keys(args).length === 0) return "";
  return ` · ${t.value.ui.arguments} ${JSON.stringify(args)}`;
}

function toolDisplayName(tool: string) {
  const zh: Record<string, string> = {
    report_inventory: "检查报告能力",
    overview: "读取全局指标",
    hotspots: "读取 CPU 热点",
    hotspot_groups: "聚合热点类别",
    hot_paths: "下钻热点子路径",
    mod_sources: "归因模组来源",
    time_windows: "读取时间窗口",
    worst_windows: "定位最坏窗口",
    entities: "统计实体分布",
    entity_chunks: "定位实体密集区块",
    heap: "分析堆内存",
    memory_gc: "分析 GC/内存",
    diagnostic_hypotheses: "生成候选结论",
    evidence_gaps: "检查证据缺口",
    raw_field: "读取原始字段",
  };
  const en: Record<string, string> = {
    report_inventory: "Inspect Report Capabilities",
    overview: "Read Overview Metrics",
    hotspots: "Read CPU Hotspots",
    hotspot_groups: "Group Hotspots",
    hot_paths: "Drill Into Hot Paths",
    mod_sources: "Attribute Mod Sources",
    time_windows: "Read Time Windows",
    worst_windows: "Find Worst Windows",
    entities: "Summarize Entities",
    entity_chunks: "Locate Dense Entity Chunks",
    heap: "Analyze Heap",
    memory_gc: "Analyze GC/Memory",
    diagnostic_hypotheses: "Build Diagnostic Hypotheses",
    evidence_gaps: "Check Evidence Gaps",
    raw_field: "Read Raw Field",
  };
  return (language.value === "zh" ? zh : en)[tool] ?? tool;
}

function toolIntent(tool: string, args: any) {
  const limit = args?.limit ? (language.value === "zh" ? `，最多 ${args.limit} 条` : `, up to ${args.limit}`) : "";
  const zh: Record<string, string> = {
    report_inventory: "先确认这份报告包含哪些可分析的数据。",
    overview: "读取 TPS、MSPT、堆内存、实体数和本地阈值告警。",
    hotspots: `读取采样调用栈中的最高 CPU 热点${limit}。`,
    hotspot_groups: `把热点按实体、区块、方块实体、IO 等类别聚合${limit}。`,
    hot_paths: `从 ${args?.category ?? "entity_tick"} 热点入口向下展开具体子路径${limit}。`,
    mod_sources: `利用 spark 的 source map 尝试归因到具体模组${limit}。`,
    time_windows: `读取 spark 的时间窗口统计${limit}。`,
    worst_windows: `按 max MSPT、median MSPT 和低 TPS 找最坏时间窗口${limit}。`,
    entities: "读取全局实体排行和世界摘要。",
    entity_chunks: `查找实体数量最高的区块和区块内实体类型${limit}。`,
    heap: `读取 heap 对象排行${limit}。`,
    memory_gc: "读取堆、内存池和 GC 聚合统计，判断是否存在 GC 暂停或频率异常。",
    diagnostic_hypotheses: "把热点、模组来源、实体区块和时间窗口交叉成候选结论。",
    evidence_gaps: "检查当前报告还能证明什么、不能证明什么，以及下一步该补采什么。",
    raw_field: `读取原始字段 ${args?.path ?? ""}。`,
  };
  const en: Record<string, string> = {
    report_inventory: "Check which data families this report contains.",
    overview: "Read TPS, MSPT, heap, entity counts, and local threshold findings.",
    hotspots: `Read the highest CPU stack hotspots${limit}.`,
    hotspot_groups: `Group hotspots by entity, chunk, block entity, IO, and related categories${limit}.`,
    hot_paths: `Drill into child frames under ${args?.category ?? "entity_tick"} hotspots${limit}.`,
    mod_sources: `Use spark source maps to attribute frames to mods${limit}.`,
    time_windows: `Read spark time-window statistics${limit}.`,
    worst_windows: `Find worst windows by max MSPT, median MSPT, and low TPS${limit}.`,
    entities: "Read global entity ranking and world summaries.",
    entity_chunks: `Find chunks with the highest entity density and their entity mixes${limit}.`,
    heap: `Read heap object rankings${limit}.`,
    memory_gc: "Read heap, memory pools, and GC aggregate statistics to detect pause/frequency anomalies.",
    diagnostic_hypotheses: "Cross-check hotspots, mod sources, entity chunks, and windows into candidate conclusions.",
    evidence_gaps: "Check what the report can prove, cannot prove, and what to capture next.",
    raw_field: `Read raw field ${args?.path ?? ""}.`,
  };
  return (language.value === "zh" ? zh : en)[tool] ?? tool;
}

function categoryDisplayName(category: string) {
  const zh: Record<string, string> = {
    other: "框架/其他",
    world_tick: "世界 tick",
    entity_tick: "实体 tick",
    entity_ai_pathfinding: "实体 AI/寻路",
    chunk_task: "区块任务",
    block_entity: "方块实体",
    commands: "命令/function",
    io: "IO",
    gc: "GC",
  };
  return language.value === "zh" ? zh[category] ?? category : category;
}

function hypothesisDisplayName(id: string) {
  if (id.startsWith("mod_source_hotspot:")) {
    const source = id.slice("mod_source_hotspot:".length);
    return language.value === "zh" ? `模组热点关联: ${source}` : `mod source hotspot: ${source}`;
  }
  const zh: Record<string, string> = {
    high_density_entity_chunk: "实体密集区块",
    chunk_task_or_generation_spike: "区块任务/生成尖峰",
    entity_tick_load: "实体 tick 负载",
    c2me_chunk_io_path: "C2ME/chunk IO 路径",
    gc_pause_possible_but_unproven: "GC 停顿待证",
    memory_gc_pressure: "GC/内存压力",
  };
  return language.value === "zh" ? zh[id] ?? id : id;
}

function confidenceDisplayName(confidence: string) {
  if (language.value !== "zh") return confidence;
  if (confidence === "high") return "高";
  if (confidence === "medium") return "中";
  if (confidence === "low") return "低";
  return confidence;
}

function parseJson(value: string): any | null {
  try {
    return JSON.parse(value);
  } catch {
    return null;
  }
}

function firstUsefulLine(value: string) {
  return value.split(/\r?\n/).map((line) => line.trim()).find(Boolean) ?? "";
}

function summarizeJson(value: any) {
  if (Array.isArray(value)) return `${value.length} items`;
  if (value && typeof value === "object") return Object.keys(value).slice(0, 8).join(", ");
  return String(value ?? "-");
}

function shortClassName(value: string) {
  const noMethod = String(value ?? "").replace(/\.[^.()]+(?::\d+)?$/, "");
  return noMethod.split(".").slice(-1)[0] || String(value ?? "-");
}

function prioritizedSources(parsed: any) {
  const combined = [...(parsed.notableSources ?? []), ...(parsed.topSources ?? [])];
  const byId = new Map<string, any>();
  for (const item of combined) {
    const key = item.sourceId ?? item.name ?? JSON.stringify(item);
    if (!byId.has(key)) byId.set(key, item);
  }
  return [...byId.values()].sort((left, right) => Number(right.maxPercent ?? 0) - Number(left.maxPercent ?? 0));
}

const InfoTip = (props: { text: string }) =>
  h(
    NTooltip,
    {},
    {
      trigger: () => h(NButton, { quaternary: true, circle: true, size: "tiny" }, { default: () => "?" }),
      default: () => props.text,
    },
  );
</script>

<template>
  <n-config-provider :theme="naiveTheme">
    <n-message-provider>
      <n-layout class="app-shell" :data-theme="themeMode">
        <header class="window-titlebar">
          <div
            class="window-title"
            data-tauri-drag-region
            @mousedown="startWindowDrag"
            @dblclick="toggleMaximizeWindow"
          >
            <div class="title-stack">
              <div class="title-row">
                <h1>BroKnowMySparkAnalyzer</h1>
                <n-tag type="success" size="small" :bordered="false">{{ status }}</n-tag>
              </div>
              <p class="eyebrow">{{ t.ui.subtitle }}</p>
            </div>
          </div>
          <n-space align="center" class="top-actions" :wrap="false" @mousedown.stop>
            <div class="control-pair">
              <n-icon class="control-icon" :component="Language" />
              <n-select v-model:value="language" class="language-select" size="small" :options="languageOptions" />
            </div>
            <div class="control-pair">
              <n-switch v-model:value="lightThemeEnabled" size="small">
                <template #checked>{{ t.ui.light }}</template>
                <template #unchecked>{{ t.ui.dark }}</template>
              </n-switch>
            </div>
            <div v-if="altPressed" class="debug-toggle">
              <span>{{ t.ui.debug }}</span>
              <n-switch v-model:value="debugMode" size="small" />
            </div>
          </n-space>
          <div class="window-controls" @pointerdown.stop @mousedown.stop>
            <button type="button" class="window-control github" title="GitHub" aria-label="Open GitHub" @pointerdown.stop.prevent="openGitHub">
              <n-icon :component="Github" />
            </button>
            <button type="button" class="window-control" title="Minimize" aria-label="Minimize" @pointerdown.stop.prevent="minimizeWindow">
              <n-icon :component="LineHorizontal120Regular" />
            </button>
            <button type="button" class="window-control" title="Maximize" aria-label="Maximize" @pointerdown.stop.prevent="toggleMaximizeWindow">
              <n-icon :component="Maximize20Regular" />
            </button>
            <button type="button" class="window-control close" title="Close" aria-label="Close" @pointerdown.stop.prevent="closeWindow">
              <n-icon :component="Dismiss20Regular" />
            </button>
          </div>
        </header>

        <n-layout has-sider class="workspace">
          <n-layout-sider class="sidebar" :width="360" bordered content-style="padding: 18px;">
            <section class="panel resizable-panel">
              <div class="panel-title">
                <h2>{{ t.ui.report }}</h2>
                <component :is="InfoTip" :text="t.ui.reportTip" />
              </div>

              <label
                class="drop-zone"
                :data-dragging="String(dragging)"
                @dragover.prevent="dragging = true"
                @dragleave="dragging = false"
                @drop="onDrop"
              >
                <input type="file" @change="(event) => handleFiles((event.target as HTMLInputElement).files ?? [])" />
                <strong>{{ t.ui.dropTitle }}</strong>
                <span>{{ t.ui.dropHint }}</span>
              </label>

              <n-divider />
              <n-input
                v-model:value="sourceInput"
                :placeholder="t.ui.sourcePlaceholder"
                clearable
              />
              <n-button
                block
                secondary
                class="mt-8"
                :loading="fetchingReport"
                :disabled="fetchingReport"
                @click="fetchRemoteReport"
              >
                {{ fetchingReport ? t.ui.fetching : t.ui.fetch }}
              </n-button>

              <n-collapse class="mt-12 optional-input">
                <n-collapse-item :title="t.ui.optionalTextInput" name="text-input">
                  <n-input
                    v-model:value="textInput"
                    type="textarea"
                    :autosize="{ minRows: 4, maxRows: 8 }"
                    :placeholder="t.ui.textPlaceholder"
                  />
                  <n-button block secondary class="mt-8" @click="analyzeText">{{ t.ui.loadText }}</n-button>
                </n-collapse-item>
              </n-collapse>
            </section>

            <section class="panel resizable-panel mt-16">
              <div class="panel-title">
                <h2>AI</h2>
                <component :is="InfoTip" :text="t.ui.aiTip" />
              </div>
              <n-select :value="providerId" :options="providerOptions" @update:value="applyProvider" />
              <n-input v-model:value="baseUrl" class="mt-10" :placeholder="t.ui.baseUrlPlaceholder" />
              <n-input v-model:value="apiKey" class="mt-10" type="password" show-password-on="click" :placeholder="t.ui.apiKeyPlaceholder" />
              <div class="model-row mt-10">
                <n-select
                  v-model:value="model"
                  :options="modelOptions"
                  :placeholder="t.ui.modelPlaceholder"
                  filterable
                  tag
                />
                <n-button :loading="fetchingModels" secondary @click="fetchModels">{{ t.ui.getModels }}</n-button>
              </div>
              <n-collapse class="mt-10 ai-advanced">
                <n-collapse-item :title="t.ui.advancedAi" name="advanced-ai">
                  <div class="field-label">
                    <span>{{ t.ui.temperature }}</span>
                    <n-text depth="3">{{ t.ui.temperatureTip }}</n-text>
                  </div>
                  <n-input-number v-model:value="temperature" class="full" :min="0" :max="2" :step="0.1" />
                </n-collapse-item>
              </n-collapse>
              <n-space class="mt-12" :wrap="false">
                <n-button :loading="testing" secondary @click="testAi">{{ t.ui.test }}</n-button>
                <n-button type="primary" :loading="busy" :disabled="!canAnalyze" @click="runAnalysis">
                  {{ t.ui.analyze }}
                </n-button>
              </n-space>
            </section>

            <section class="panel resizable-panel followup-panel sidebar-followup mt-16">
              <div class="panel-title">
                <h2>{{ t.ui.followUp }}</h2>
                <n-tag size="small" :bordered="false">{{ followUps.length }}</n-tag>
              </div>
              <div class="followup-list">
                <div v-if="followUps.length === 0" class="empty compact-empty">{{ aiOutput ? t.ui.followUpPlaceholder : t.ui.noDiagnosis }}</div>
                <article
                  v-for="(item, index) in followUps"
                  :key="`${item.role}-${index}`"
                  class="followup-message"
                  :data-role="item.role"
                >
                  <strong>{{ item.role === "user" ? "你" : "AI" }}</strong>
                  <div class="markdown-body followup-content" v-html="renderFollowUp(item.content)"></div>
                </article>
              </div>
              <div class="followup-compose">
                <n-input
                  v-model:value="followUpInput"
                  type="textarea"
                  :autosize="{ minRows: 2, maxRows: 5 }"
                  :placeholder="t.ui.followUpPlaceholder"
                  :disabled="!aiOutput || followUpBusy"
                  @keydown.ctrl.enter.prevent="sendFollowUp"
                />
                <n-button type="primary" :loading="followUpBusy" :disabled="!canAskFollowUp" @click="sendFollowUp">
                  {{ t.ui.ask }}
                </n-button>
              </div>
            </section>
          </n-layout-sider>

          <n-layout class="main-content" content-style="padding: 18px;">
            <section class="metrics-row">
              <n-statistic :label="t.ui.type" :value="report?.kind ?? '-'" />
              <n-statistic label="TPS 1m" :value="summary?.tps1m === undefined ? '-' : formatNumber(summary.tps1m)" />
              <n-statistic label="MSPT P95/Max" :value="`${formatNumber(summary?.msptP95)} / ${formatNumber(summary?.msptMax)}`" />
              <n-statistic :label="t.ui.heap" :value="`${formatBytes(summary?.heapUsedBytes)} / ${formatBytes(summary?.heapMaxBytes)}`" />
              <n-statistic :label="t.ui.entities" :value="summary?.entityCount === undefined ? '-' : formatNumber(summary.entityCount)" />
            </section>

            <section class="content-grid">
              <div class="panel resizable-panel status-panel">
                <div class="panel-title compact">
                  <h2>{{ t.ui.reportStatus }}</h2>
                  <n-button size="small" secondary @click="clearAll">{{ t.ui.clear }}</n-button>
                </div>
                <n-text depth="3">{{ report?.source ?? t.ui.noReport }}</n-text>
                <div class="finding-list mt-12">
                  <n-alert
                    v-for="finding in summary?.findings ?? []"
                    :key="finding.title"
                    :type="severityType(finding.severity)"
                    :title="finding.title"
                  >
                    {{ finding.detail }}
                  </n-alert>
                </div>
              </div>

              <div class="panel resizable-panel trace-panel">
                <div class="panel-title compact">
                  <h2>{{ t.ui.trace }}</h2>
                  <n-tag size="small" :type="debugMode ? 'warning' : 'default'" :bordered="false">
                    {{ debugMode ? t.ui.raw : t.ui.summary }}
                  </n-tag>
                </div>
                <div v-if="traces.length === 0" class="empty">{{ t.ui.noTrace }}</div>
                <div v-else class="trace-list">
                  <article
                    v-for="(trace, index) in traces"
                    :key="`${trace.round}-${index}`"
                    class="trace-item"
                    :data-role="traceRoleClass(trace)"
                  >
                    <div class="trace-marker">{{ index + 1 }}</div>
                    <div class="trace-body">
                      <div class="trace-head">
                        <div>
                          <h3>{{ traceTitle(trace) }}</h3>
                          <span>{{ traceSubhead(trace) }}</span>
                        </div>
                        <n-tag :type="traceType(trace)" size="small" round>
                          {{ t.ui.round }} {{ trace.round }}
                        </n-tag>
                      </div>
                      <p class="trace-summary">{{ traceSummary(trace) }}</p>
                      <pre v-if="debugMode">{{ trace.content }}</pre>
                    </div>
                  </article>
                </div>
              </div>
            </section>

            <section class="panel resizable-panel diagnosis-panel mt-16">
              <div class="panel-title">
                <h2>{{ t.ui.diagnosis }}</h2>
                <n-space :wrap="false" size="small">
                  <n-button size="small" secondary :disabled="!aiOutput" @click="diagnosisFullscreen = true">
                    {{ t.ui.fullscreen }}
                  </n-button>
                  <n-button size="small" secondary :disabled="!aiOutput" @click="exportMarkdown">
                    {{ t.ui.exportMd }}
                  </n-button>
                  <n-button size="small" type="primary" secondary :disabled="!aiOutput" @click="exportDiagnosisImage">
                    {{ t.ui.exportImage }}
                  </n-button>
                </n-space>
              </div>
              <div ref="diagnosisRef" class="markdown-body export-surface" v-html="renderedMarkdown"></div>
            </section>

          </n-layout>
        </n-layout>
      </n-layout>

      <n-modal v-model:show="diagnosisFullscreen" display-directive="show">
        <section class="fullscreen-diagnosis" :data-theme="themeMode">
          <header>
            <h2>{{ t.ui.diagnosis }}</h2>
            <n-space :wrap="false" size="small">
              <n-button size="small" secondary @click="exportMarkdown">{{ t.ui.exportMd }}</n-button>
              <n-button size="small" type="primary" secondary @click="exportDiagnosisImage">{{ t.ui.exportImage }}</n-button>
              <n-button size="small" @click="diagnosisFullscreen = false">{{ t.ui.close }}</n-button>
            </n-space>
          </header>
          <div class="markdown-body fullscreen-markdown" v-html="renderedMarkdown"></div>
        </section>
      </n-modal>
    </n-message-provider>
  </n-config-provider>
</template>
