import fs from "node:fs/promises";
import assert from "node:assert/strict";
import init, { Analyzer } from "../public/bkmsa-wasm/bkmsa_wasm.js";

const bytes = await fs.readFile(new URL("../public/bkmsa-wasm/bkmsa_wasm_bg.wasm", import.meta.url));
await init({ module_or_path: bytes });

const analyzer = new Analyzer();
const report = analyzer.loadTextReport("Can't keep up!", "wasm-smoke");
const overview = analyzer.executeTool(report.reportId, "overview", {});

if (report.kind !== "text" || overview.source !== "wasm-smoke") {
  throw new Error(`WASM adapter contract failed: ${JSON.stringify({ report, overview })}`);
}
if (analyzer.cancelAnalysis(report.reportId) !== false) {
  throw new Error("idle report unexpectedly had an active analysis task");
}

// Exercise the browser facade with a paused provider, without external credentials.
const originalFetch = globalThis.fetch;
globalThis.window = globalThis;
globalThis.Window = class { static [Symbol.hasInstance](value) { return value === globalThis; } };
const traces = [];
let requests = 0;
let notifyPaused;
let resume;
const paused = new Promise((resolve) => { notifyPaused = resolve; });
const responseGate = new Promise((resolve) => { resume = resolve; });
const response = (content) => new Response(JSON.stringify({ choices: [{ message: { content } }] }), {
  headers: { "Content-Type": "application/json" },
});
globalThis.fetch = async () => {
  requests += 1;
  if (requests === 1) { notifyPaused(); await responseGate; }
  return response("# 结论\n当前报告无法唯一定位。\n# 优先处理\n卡顿时采集 CPU profile。\n# 判断依据\n只有文本记录，无采样证据。\n# 复测与补采\n同人数同区域补采 spark 报告。");
};
try {
  const config = { base_url: "https://provider.invalid/v1", api_key: "mock", model: "mock", temperature: 0.2, timeout_secs: 5 };
  let finished = false;
  const pending = analyzer.runAnalysis(report.reportId, config, (trace) => traces.push(trace));
  pending.then(() => { finished = true; }, () => { finished = true; });
  await Promise.race([paused, pending.then(() => { throw new Error("analysis finished before provider paused"); })]);
  assert.equal(finished, false);
  assert.deepEqual(traces.filter((trace) => trace.role === "tool").map((trace) => trace.title), [
    "Tool: report_inventory", "Tool: overview", "Tool: evidence_gaps",
  ]);
  resume();
  const result = await pending;
  assert.equal(result.rounds, 1);
  assert.equal(result.reached_round_limit, false);
  assert.deepEqual(traces, result.traces);
  assert.equal(analyzer.cancelAnalysis(report.reportId), false);

  // Even a provider response delivered after cancellation cannot publish traces.
  const cancelPaused = new Promise((resolve) => { notifyPaused = resolve; });
  let resolveCanceled;
  globalThis.fetch = () => new Promise((resolve) => { resolveCanceled = resolve; notifyPaused(); });
  const canceledTraces = [];
  const canceled = analyzer.runAnalysis(report.reportId, config, (trace) => canceledTraces.push(trace));
  const rejected = assert.rejects(canceled, (error) => String(error).includes("分析已中止"));
  await cancelPaused;
  assert.equal(analyzer.cancelAnalysis(report.reportId), true);
  await rejected;
  resolveCanceled(response('{"tool":"overview","args":{}}'));
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(canceledTraces.length, 3);

  // Prefetched tools emit consecutive traces in one poll. A callback may
  // synchronously cancel or release its report between those emissions.
  for (const stage of ["prefetch-tool", "final-answer"]) for (const action of ["cancel", "release"]) {
    const current = analyzer.loadTextReport("Can't keep up!", "callback-cancellation");
    globalThis.fetch = async () => response(result.diagnosis);
    const callbackTraces = [];
    let stopped = false;
    const pending = analyzer.runAnalysis(current.reportId, config, (trace) => {
      callbackTraces.push(trace);
      const shouldStop = stage === "prefetch-tool"
        ? trace.title === "Tool: overview"
        : trace.role === "assistant" && callbackTraces.some((item) => item.title === "Tool: evidence_gaps");
      if (shouldStop) {
        if (action === "cancel") stopped = analyzer.cancelAnalysis(current.reportId);
        else { analyzer.releaseReport(current.reportId); stopped = true; }
      }
    });
    await assert.rejects(pending, (error) => String(error).includes("分析已中止"));
    assert.equal(stopped, true);
    assert.equal(callbackTraces.at(-1).title, stage === "prefetch-tool" ? "Tool: overview" : "AI");
    analyzer.releaseReport(current.reportId);
  }
} finally {
  globalThis.fetch = originalFetch;
  delete globalThis.window;
  delete globalThis.Window;
}

analyzer.releaseReport(report.reportId);
console.log(JSON.stringify({
  reportId: report.reportId,
  kind: report.kind,
  overviewSource: overview.source,
}));
