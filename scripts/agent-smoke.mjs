#!/usr/bin/env node

// Compatibility wrapper only. Every tool invocation is delegated to bkmsa-core through the CLI.
import { runBkmsa } from "./bkmsa-wrapper.mjs";

const [file, ...unexpected] = process.argv.slice(2);
if (!file || unexpected.length > 0) {
  console.error("usage: node scripts/agent-smoke.mjs <report>");
  console.error("preferred: bkmsa inventory <report> && bkmsa tool <report> <tool>");
  process.exit(2);
}

if (file === "-") {
  console.error("stdin is not supported by this multi-command wrapper; save it to a report file first");
  process.exit(2);
}

const report = file.startsWith("-") ? `./${file}` : file;

console.error("[deprecated] this script is a thin deterministic-tool smoke wrapper around `bkmsa`.");

const calls = [
  ["inventory", report, "--format", "json"],
  ...[
    ["overview"],
    ["environment"],
    ["hotspots", "--limit", "12"],
    ["hotspot_groups", "--limit", "12"],
    ["hot_paths", "--category", "auto", "--limit", "16"],
    ["mod_sources", "--limit", "12"],
    ["time_windows", "--limit", "10"],
    ["worst_windows", "--limit", "6"],
    ["entities"],
    ["entity_chunks", "--limit", "12"],
    ["heap", "--limit", "12"],
    ["memory_gc"],
    ["evidence_links", "--limit", "16"],
    ["diagnostic_hypotheses"],
    ["evidence_gaps"],
  ].map(([tool, ...args]) => ["tool", report, tool, ...args, "--format", "json"]),
];

for (const args of calls) {
  const status = runBkmsa(args);
  if (status !== 0) process.exit(status);
}
