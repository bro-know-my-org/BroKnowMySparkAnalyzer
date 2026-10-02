import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { createTauriSparkAnalyzerAdapter } from "../src/tauri";
import type { AgentTrace, AnalysisResult } from "../src/adapter";

const ipc = vi.fn<(name: string, args?: Record<string, unknown>) => Promise<unknown>>().mockResolvedValue({});
beforeEach(() => vi.stubGlobal("__TAURI_INTERNALS__", {
  invoke: ipc,
}));

afterEach(() => { ipc.mockReset().mockResolvedValue({}); vi.unstubAllGlobals(); });

it("addresses text-report IPC using the generated bkmsa-tauri permission namespace", async () => {
  const adapter = createTauriSparkAnalyzerAdapter();
  await adapter.loadTextReport("local report", "acceptance");
  expect(ipc).toHaveBeenCalledExactlyOnceWith("plugin:bkmsa-tauri|analyzer_load_text_report", {
    request: { text: "local report", source: "acceptance" },
  }, undefined);
});

it("delivers ordered traces before analysis resolves and ignores late messages", async () => {
  let dispatch!: (message: unknown) => void;
  vi.stubGlobal("__TAURI_INTERNALS__", {
    invoke: ipc,
    transformCallback: (callback: typeof dispatch) => { dispatch = callback; return 7; },
    unregisterCallback: vi.fn(),
  });
  let resolve!: (result: AnalysisResult) => void;
  ipc.mockReturnValueOnce(new Promise<AnalysisResult>((accept) => { resolve = accept; }));
  const traces: AgentTrace[] = [];
  const first: AgentTrace = { round: 0, role: "tool", title: "inventory", content: "{}" };
  const second: AgentTrace = { ...first, round: 1, title: "overview" };
  const pending = createTauriSparkAnalyzerAdapter().runAnalysis("report-1", {
    base_url: "http://localhost/v1", api_key: "test", model: "mock", temperature: 0.2,
  }, (trace) => traces.push(trace));
  await vi.waitFor(() => expect(ipc).toHaveBeenCalledOnce());
  expect(ipc).toHaveBeenCalledWith("plugin:bkmsa-tauri|analyzer_run_analysis", {
    request: expect.objectContaining({ report_id: "report-1" }), onTrace: expect.anything(),
  }, undefined);
  expect(JSON.stringify(ipc.mock.calls[0][1]?.onTrace)).toBe('"__CHANNEL__:7"');
  dispatch({ index: 1, message: second });
  expect(traces).toEqual([]);
  dispatch({ index: 0, message: first });
  expect(traces).toEqual([first, second]);
  resolve({ diagnosis: "done", traces: [first, second] });
  await pending;
  dispatch({ index: 2, message: first });
  expect(traces).toEqual([first, second]);
});

it("submits analysis before an immediately following cancel request", async () => {
  let started = false;
  let resolve!: (result: AnalysisResult) => void;
  ipc.mockImplementation((name) => {
    if (name.endsWith("analyzer_run_analysis")) {
      started = true;
      return new Promise<AnalysisResult>((accept) => { resolve = accept; });
    }
    return Promise.resolve(started);
  });
  const adapter = createTauriSparkAnalyzerAdapter();
  const pending = adapter.runAnalysis("report-1", {
    base_url: "https://provider.invalid/v1", api_key: "test", model: "mock", temperature: 0.2,
  });
  const canceled = await adapter.cancelAnalysis("report-1");
  await vi.waitFor(() => expect(started).toBe(true));
  resolve({ diagnosis: "done", traces: [] });
  await pending;
  expect(canceled).toBe(true);
  expect(ipc.mock.calls.map(([name]) => name)).toEqual([
    "plugin:bkmsa-tauri|analyzer_run_analysis", "plugin:bkmsa-tauri|analyzer_cancel_analysis",
  ]);
});
