import { expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { createTauriSparkAnalyzerAdapter } from "../src/tauri";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn().mockResolvedValue({}) }));

it("addresses text-report IPC using the generated bkmsa-tauri permission namespace", async () => {
  const adapter = createTauriSparkAnalyzerAdapter();
  await adapter.loadTextReport("local report", "acceptance");
  expect(invoke).toHaveBeenCalledExactlyOnceWith("plugin:bkmsa-tauri|analyzer_load_text_report", {
    request: { text: "local report", source: "acceptance" },
  });
});
