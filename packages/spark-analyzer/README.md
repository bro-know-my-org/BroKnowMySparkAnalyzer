# @bro-know-my/spark-analyzer

Reusable Vue 3 Spark report analyzer view used by BroKnowMy apps.

The package contains the analyzer UI and report/AI logic. Host applications provide platform-specific behavior through an adapter, so the package can run inside different shells such as a standalone Tauri app or BroKnowMyToolbox.

## Install

```bash
pnpm add @bro-know-my/spark-analyzer
```

Peer dependencies are expected to be provided by the host app:

```bash
pnpm add vue naive-ui @vicons/fa @vicons/fluent
```

## Usage

```vue
<script setup lang="ts">
import { SparkAnalyzerView, type SparkAnalyzerAdapter } from "@bro-know-my/spark-analyzer";
import "@bro-know-my/spark-analyzer/style.css";

const adapter: SparkAnalyzerAdapter = {
  fetchReportFromUrl: async (source) => {
    throw new Error(`fetchReportFromUrl is not implemented: ${source}`);
  },
  pickSavePath: async () => null,
  saveExportFile: async () => {},
  openUrl: async (url) => window.open(url, "_blank"),
  callAiChat: async () => {
    throw new Error("callAiChat is not implemented");
  },
  testAiConnection: async () => "OK",
  listAiModels: async () => [],
};
</script>

<template>
  <SparkAnalyzerView
    :adapter="adapter"
    language="zh"
    embedded
  />
</template>
```

## Adapter Boundary

The package does not call Tauri APIs directly. Hosts should implement the adapter with their own native commands, HTTP layer, dialog APIs, and AI provider policy.

`debug` can be controlled by the host:

```vue
<SparkAnalyzerView :adapter="adapter" :debug="debugEnabled" embedded />
```

If `debug` is not passed, the analyzer keeps its standalone debug behavior.
