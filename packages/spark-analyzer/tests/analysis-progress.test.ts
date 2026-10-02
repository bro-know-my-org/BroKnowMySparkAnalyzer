import { afterEach, expect, it, vi } from "vitest";
import { flushPromises, shallowMount, type VueWrapper } from "@vue/test-utils";
import { nextTick } from "vue";
import { NButton, NInput } from "naive-ui";
import SparkAnalyzerView from "../src/SparkAnalyzerView.vue";
import type { AgentTrace, AnalysisResult, SparkAnalyzerAdapter } from "../src/adapter";

vi.mock("naive-ui", async (importOriginal) => ({
  ...await importOriginal<typeof import("naive-ui")>(),
  createDiscreteApi: () => ({ message: { success: vi.fn(), warning: vi.fn(), error: vi.fn() } }),
}));

let wrapper: VueWrapper;
afterEach(() => { wrapper?.unmount(); localStorage.clear(); });

async function startAnalysis() {
  let onTrace!: (trace: AgentTrace) => void;
  let resolve!: (result: AnalysisResult) => void;
  let reject!: (error: Error) => void;
  const result = new Promise<AnalysisResult>((accept, fail) => { resolve = accept; reject = fail; });
  let reportNumber = 0;
  const adapter = {
    loadReportBytes: vi.fn(), loadTextReport: vi.fn(),
    fetchReport: vi.fn().mockImplementation(async () => ({
      reportId: `report-${++reportNumber}`, kind: "text", source: "test", summary: { title: "test", findings: [] },
    })),
    executeTool: vi.fn().mockResolvedValue({}),
    runAnalysis: vi.fn<SparkAnalyzerAdapter["runAnalysis"]>().mockImplementation((_id, _config, callback) => {
      onTrace = callback!;
      return result;
    }),
    cancelAnalysis: vi.fn().mockResolvedValue(true), releaseReport: vi.fn().mockResolvedValue(undefined),
    loadApiKey: vi.fn().mockResolvedValue(null), storeApiKey: vi.fn(), deleteApiKey: vi.fn(),
    askFollowUp: vi.fn(), testAiConnection: vi.fn(), listAiModels: vi.fn(),
    pickSavePath: vi.fn(), saveExportFile: vi.fn(), openUrl: vi.fn(),
  } satisfies SparkAnalyzerAdapter;
  wrapper = shallowMount(SparkAnalyzerView, {
    props: { adapter, language: "en", embedded: true }, global: { renderStubDefaultSlot: true },
  });
  await flushPromises();
  wrapper.findAllComponents(NInput).find((field) => String(field.props("placeholder")).includes("spark viewer"))!
    .vm.$emit("update:value", "test-report");
  await nextTick();
  async function click(label: string) {
    const button = wrapper.findAllComponents(NButton).find((item) => item.text() === label);
    expect(button, `Missing button ${label}`).toBeDefined();
    button!.vm.$emit("click");
    await flushPromises();
  }
  await click("Fetch URL");
  await click("Agent Analyze");
  expect(adapter.runAnalysis).toHaveBeenCalledOnce();
  return { onTrace, resolve, reject, click, adapter };
}

const trace: AgentTrace = { round: 0, role: "tool", title: "Tool: report_inventory", content: "{}" };

it("renders each trace while the report is pending and reconciles without duplicates", async () => {
  const run = await startAnalysis();
  run.onTrace(trace);
  await nextTick();
  expect(wrapper.findAll(".trace-item")).toHaveLength(1);
  expect(wrapper.find(".diagnosis-panel").text()).not.toContain("Final diagnosis");
  const second = { ...trace, round: 1, title: "Tool: overview" };
  run.onTrace(second);
  await nextTick();
  expect(wrapper.findAll(".trace-item")).toHaveLength(2);
  run.resolve({ diagnosis: "Final diagnosis", traces: [trace, second] });
  await flushPromises();
  expect(wrapper.findAll(".trace-item")).toHaveLength(2);
  expect(wrapper.find(".diagnosis-panel").text()).toContain("Final diagnosis");
  run.onTrace(trace);
  await nextTick();
  expect(wrapper.findAll(".trace-item")).toHaveLength(2);
});

for (const action of ["cancel", "replace", "unmount", "fail"] as const) {
  it(`ignores old traces after ${action}`, async () => {
    const run = await startAnalysis();
    run.onTrace(trace);
    await nextTick();
    if (action === "cancel") await run.click("Stop");
    if (action === "replace") await run.click("Fetch URL");
    if (action === "unmount") wrapper.unmount();
    if (action === "fail") { run.reject(new Error("provider failed")); await flushPromises(); }
    const count = wrapper.findAll(".trace-item").length;
    run.onTrace({ ...trace, title: "stale" });
    await nextTick();
    expect(wrapper.findAll(".trace-item")).toHaveLength(count);
    if (action !== "fail") {
      run.resolve({ diagnosis: "stale diagnosis", traces: [trace] });
      await flushPromises();
      expect(wrapper.text()).not.toContain("stale diagnosis");
    }
  });
}
