<template>
  <div class="standalone-shell">
    <header class="standalone-titlebar">
      <div class="standalone-title" data-tauri-drag-region @mousedown="startWindowDrag" @dblclick="toggleMaximizeWindow">
        <div class="title-stack">
          <div class="title-row">
            <h1>BroKnowMySparkAnalyzer</h1>
          </div>
          <p class="eyebrow">Spark Agent Workbench</p>
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

    <SparkAnalyzerView class="standalone-content" :adapter="sparkAnalyzerAdapter" embedded />
  </div>
</template>

<script setup lang="ts">
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Dismiss20Regular, LineHorizontal120Regular, Maximize20Regular } from "@vicons/fluent";
import { NIcon } from "naive-ui";
import { SparkAnalyzerView } from "../packages/spark-analyzer/src";
import { sparkAnalyzerAdapter } from "./sparkAnalyzerAdapter";

const appWindow = getCurrentWindow();

function minimizeWindow() {
  void appWindow.minimize();
}

function toggleMaximizeWindow() {
  void appWindow.toggleMaximize();
}

function closeWindow() {
  void appWindow.close();
}

function startWindowDrag(event: MouseEvent) {
  if (event.button !== 0 || event.detail > 1) return;
  void appWindow.startDragging();
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
  min-width: 980px;
  background: var(--page);
}

.standalone-shell {
  display: flex;
  flex-direction: column;
  height: 100vh;
  min-height: 0;
  overflow: hidden;
  background: var(--page);
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
</style>
