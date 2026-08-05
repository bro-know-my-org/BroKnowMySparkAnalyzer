# @bro-know-my/spark-analyzer

[中文](#中文) | [English](#english)

## 中文

`@bro-know-my/spark-analyzer` 是 `bkmsa` 的可复用 Vue 3 UI。它有意不包含报告解析器、诊断规则、agent 循环或 AI transport；这些能力由 Rust `bkmsa-core` / `bkmsa-agent` 提供，并由宿主通过 `SparkAnalyzerAdapter` 暴露。

大报告保存在 Tauri 或 `bkmsa-wasm` 后端，UI 只持有不透明的 `LoadedReport.reportId` 和摘要。这条边界用于保证 CLI、桌面端和 Web 端共享同一套分析语义。

### 安装

```bash
pnpm add @bro-know-my/spark-analyzer
pnpm add vue naive-ui @vicons/fa @vicons/fluent
```

### 使用

```vue
<script setup lang="ts">
import {
  SparkAnalyzerView,
  type SparkAnalyzerAdapter,
} from "@bro-know-my/spark-analyzer";
import "@bro-know-my/spark-analyzer/style.css";

const adapter: SparkAnalyzerAdapter = {
  loadReportBytes: (bytes, source, hint) => backend.loadReportBytes(bytes, source, hint),
  loadTextReport: (text, source) => backend.loadTextReport(text, source),
  fetchReport: (source) => backend.fetchReport(source),
  executeTool: (reportId, tool, args) => backend.executeTool(reportId, tool, args),
  runAnalysis: (reportId, config) => backend.runAnalysis(reportId, config),
  cancelAnalysis: (reportId) => backend.cancelAnalysis(reportId),
  askFollowUp: (reportId, config, traces, diagnosis, history, question) =>
    backend.askFollowUp(reportId, config, traces, diagnosis, history, question),
  releaseReport: (reportId) => backend.releaseReport(reportId),
  loadApiKey: () => backend.loadApiKey(),
  storeApiKey: (apiKey) => backend.storeApiKey(apiKey),
  deleteApiKey: () => backend.deleteApiKey(),
  testAiConnection: (config) => backend.testAiConnection(config),
  listAiModels: (config) => backend.listAiModels(config),
  pickSavePath: async () => null,
  saveExportFile: async () => {},
  openUrl: async (url) => window.open(url, "_blank", "noopener,noreferrer"),
};
</script>

<template>
  <SparkAnalyzerView :adapter="adapter" language="zh" embedded />
</template>
```

UI 不直接调用 Tauri 或 WASM API。仓库内的 standalone host 在桌面端把 adapter 映射到 `analyzer_*` Tauri commands，在浏览器端延迟加载 `bkmsa-wasm`。

Tauri 2 宿主推荐直接使用配套的 `bkmsa-tauri` crate 和包内 adapter：

```rust
tauri::Builder::default()
    .plugin(bkmsa_tauri::init())
```

在 capability 中加入 `"bkmsa-tauri:default"`，然后：

```ts
import { createTauriSparkAnalyzerAdapter } from "@bro-know-my/spark-analyzer/tauri";

const adapter = createTauriSparkAnalyzerAdapter();
```

宿主仍需安装并注册 `@tauri-apps/plugin-dialog` 和 `@tauri-apps/plugin-opener`，用于导出路径选择和打开外部链接。

宿主可以显式控制 debug：

```vue
<SparkAnalyzerView :adapter="adapter" :debug="debugEnabled" embedded />
```

不传 `debug` 时，组件保留 standalone 默认行为。

### 构建

```bash
pnpm --dir packages/spark-analyzer build
```

这是纯 UI 构建，不会生成 Rust/WASM 后端。完整 Web 应用应先在仓库根目录运行 `pnpm run build:wasm`，再运行 `pnpm run build:web`。

## English

`@bro-know-my/spark-analyzer` is the reusable Vue 3 UI for `bkmsa`. It intentionally contains no report parser, diagnostic rules, agent loop, or AI transport. Those capabilities come from the Rust `bkmsa-core` / `bkmsa-agent` crates and are exposed by the host through `SparkAnalyzerAdapter`.

Large reports stay inside the Tauri or `bkmsa-wasm` backend. The UI holds only an opaque `LoadedReport.reportId` and a summary. This boundary keeps the CLI, desktop app, and Web app on the same analysis semantics.

### Install

```bash
pnpm add @bro-know-my/spark-analyzer
pnpm add vue naive-ui @vicons/fa @vicons/fluent
```

### Usage

```vue
<script setup lang="ts">
import {
  SparkAnalyzerView,
  type SparkAnalyzerAdapter,
} from "@bro-know-my/spark-analyzer";
import "@bro-know-my/spark-analyzer/style.css";

const adapter: SparkAnalyzerAdapter = {
  loadReportBytes: (bytes, source, hint) => backend.loadReportBytes(bytes, source, hint),
  loadTextReport: (text, source) => backend.loadTextReport(text, source),
  fetchReport: (source) => backend.fetchReport(source),
  executeTool: (reportId, tool, args) => backend.executeTool(reportId, tool, args),
  runAnalysis: (reportId, config) => backend.runAnalysis(reportId, config),
  cancelAnalysis: (reportId) => backend.cancelAnalysis(reportId),
  askFollowUp: (reportId, config, traces, diagnosis, history, question) =>
    backend.askFollowUp(reportId, config, traces, diagnosis, history, question),
  releaseReport: (reportId) => backend.releaseReport(reportId),
  loadApiKey: () => backend.loadApiKey(),
  storeApiKey: (apiKey) => backend.storeApiKey(apiKey),
  deleteApiKey: () => backend.deleteApiKey(),
  testAiConnection: (config) => backend.testAiConnection(config),
  listAiModels: (config) => backend.listAiModels(config),
  pickSavePath: async () => null,
  saveExportFile: async () => {},
  openUrl: async (url) => window.open(url, "_blank", "noopener,noreferrer"),
};
</script>

<template>
  <SparkAnalyzerView :adapter="adapter" language="zh" embedded />
</template>
```

The UI does not call Tauri or WASM APIs directly. The repository's standalone host maps the adapter to `analyzer_*` Tauri commands on desktop and lazily loads `bkmsa-wasm` in browsers.

Tauri 2 hosts should use the matching `bkmsa-tauri` crate and the packaged adapter:

```rust
tauri::Builder::default()
    .plugin(bkmsa_tauri::init())
```

Add `"bkmsa-tauri:default"` to the capability, then:

```ts
import { createTauriSparkAnalyzerAdapter } from "@bro-know-my/spark-analyzer/tauri";

const adapter = createTauriSparkAnalyzerAdapter();
```

The host must also install and register `@tauri-apps/plugin-dialog` and `@tauri-apps/plugin-opener` for export path selection and external URLs.

The host may control debug mode explicitly:

```vue
<SparkAnalyzerView :adapter="adapter" :debug="debugEnabled" embedded />
```

When `debug` is omitted, the component keeps its standalone default behavior.

### Build

```bash
pnpm --dir packages/spark-analyzer build
```

This builds only the UI and does not produce a Rust/WASM backend. For the complete Web app, run `pnpm run build:wasm` and then `pnpm run build:web` from the repository root.
