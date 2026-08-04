#!/usr/bin/env node

// Compatibility wrapper only. Parsing and summaries live in bkmsa-core.
import { runBkmsa } from "./bkmsa-wrapper.mjs";

const files = process.argv.slice(2);
if (files.length === 0) {
  console.error("usage: node scripts/inspect-sparkprofile.mjs <report>...");
  console.error("preferred: bkmsa inspect <report>");
  process.exit(1);
}

console.error("[deprecated] use `bkmsa inspect <report>`; this script is only a thin CLI wrapper.");

for (const file of files) {
  const status = runBkmsa(["inspect", "--format", "terminal", "--", file]);
  if (status !== 0) process.exit(status);
}
