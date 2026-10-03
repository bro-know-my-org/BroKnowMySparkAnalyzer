<template>
  <div class="standalone-shell">
    <header v-if="isDesktop" class="standalone-titlebar">
      <div class="standalone-title" data-tauri-drag-region @mousedown="startWindowDrag" @dblclick="toggleMaximizeWindow">
        <div class="title-stack">
          <div class="title-row">
            <h1>BroKnowMySparkAnalyzer</h1>
          </div>
        </div>
      </div>
      <div class="window-controls" @pointerdown.stop @mousedown.stop>
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

    <SparkAnalyzerView class="standalone-content" :adapter="sparkAnalyzerAdapter" :embedded="isDesktop" />
  </div>
</template>

<script setup lang="ts">
import { Dismiss20Regular, LineHorizontal120Regular, Maximize20Regular } from "@vicons/fluent";
import { NIcon } from "naive-ui";
import { SparkAnalyzerView } from "../packages/spark-analyzer/src";
import { isTauriRuntime, sparkAnalyzerAdapter } from "./sparkAnalyzerAdapter";

const isDesktop = isTauriRuntime();
let appWindowPromise: ReturnType<typeof loadAppWindow> | undefined;

async function loadAppWindow() {
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  return getCurrentWindow();
}

function appWindow() {
  appWindowPromise ??= loadAppWindow();
  return appWindowPromise;
}

function minimizeWindow() {
  void appWindow().then((window) => window.minimize());
}

function toggleMaximizeWindow() {
  void appWindow().then((window) => window.toggleMaximize());
}

function closeWindow() {
  void appWindow().then((window) => window.close());
}

function startWindowDrag(event: MouseEvent) {
  if (event.button !== 0 || event.detail > 1) return;
  void appWindow().then((window) => window.startDragging());
}
</script>

<style scoped>
:global(html),
:global(body),
:global(#app) {
  width: 100%;
  height: 100%;
  overflow: hidden;
}

:global(body) {
  margin: 0;
  min-width: 0;
  background: var(--page);
}

.standalone-shell {
  font-family: Inter, "Segoe UI", "Microsoft YaHei", system-ui, sans-serif;
  --page: #f6f7f9;
  --surface: #ffffff;
  --border: #e3e7ed;
  --text: #202b3d;
  --muted: #66748a;
  display: flex;
  flex-direction: column;
  height: 100vh;
  min-height: 0;
  overflow: hidden;
  background: var(--page);
}

.standalone-shell:has(.app-shell[data-theme="dark"]) {
  --page: #141820;
  --surface: #1b2029;
  --border: #303846;
  --text: #e5eaf3;
  --muted: #a0abbd;
}

.standalone-titlebar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 42px;
  flex: 0 0 42px;
  border-bottom: 1px solid var(--border);
  background: color-mix(in srgb, var(--surface) 94%, transparent);
}

.standalone-title {
  display: flex;
  min-width: 0;
  flex: 1;
  align-self: stretch;
  align-items: center;
  padding-left: 14px;
  user-select: none;
}

.standalone-content {
  min-height: 0;
  flex: 1 1 auto;
}

.title-stack h1 {
  margin: 0;
  color: var(--text, #202b3d);
  font-size: 13px;
  line-height: 1.4;
}

.window-controls { display: flex; align-self: stretch; }
.window-control {
  width: 44px;
  border: 0;
  background: transparent;
  color: var(--muted, #66748a);
  font-size: 15px;
}
.window-control:hover { background: var(--border); }
.window-control.close:hover { color: white; background: #c42b1c; }

@media (max-width: 820px) {
  :global(html),
  :global(body),
  :global(#app) {
    height: auto;
    min-height: 100%;
    overflow: auto;
  }

  .standalone-shell {
    height: auto;
    min-height: 100vh;
    min-height: 100dvh;
    overflow: visible;
  }
}
</style>
