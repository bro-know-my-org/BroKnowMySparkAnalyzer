import fs from "node:fs/promises";
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

analyzer.releaseReport(report.reportId);
console.log(JSON.stringify({
  reportId: report.reportId,
  kind: report.kind,
  overviewSource: overview.source,
}));
