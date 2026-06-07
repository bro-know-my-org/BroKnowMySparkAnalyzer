# BroKnowMySparkAnalyzer

Tauri desktop app for inspecting Minecraft spark reports locally. The AI side runs as a small report-analysis agent: it sees the available report tools, requests the data it needs, and then writes a Markdown diagnosis from the evidence it collected.

## Current scope

- Import local spark protobuf files:
  - `.sparkprofile` from CPU profiler fallback save.
  - `.sparkheap` from heap summary fallback save.
  - raw health protobuf files if saved separately.
- Paste a `spark.lucko.me` viewer URL, `spark-usercontent.lucko.me` URL, or raw bytebin key.
- Paste plain text logs or manually copied report notes.
- Decode protobuf with the schema copied from `lucko/spark`.
- Generate local findings for TPS, MSPT, CPU, heap, entities, GC, CPU hotspots, and heap object hotspots.
- Run an AI tool loop over the report instead of sending one large hand-built prompt.
- Render the final diagnosis as Markdown and show the agent tool trace.
- Default to Chinese and dark mode, with top-bar language and light/dark theme switches.
- Keep raw agent/tool payloads behind a Debug switch. Normal mode shows concise tool-call summaries.
- Diagnosis panel supports fullscreen viewing, Markdown export, and PNG image export.
- Main panels and metric cards can be manually resized vertically from the browser resize handle.
- Test AI provider connectivity from the settings panel.
- Fetch OpenAI-compatible model lists from `/models`; the model selector remains manually editable for providers that do not expose model listing.
- Keep less-used AI knobs such as temperature under a collapsed Advanced AI Settings section. Default temperature is `0.2`.
- Remote report fetching shows a loading state on the fetch button.
- After a diagnosis is generated, the follow-up panel can ask the AI detailed questions using the current report, final diagnosis, and collected tool evidence as context.
- Call OpenAI-compatible `/chat/completions` providers with configurable `API Key`, `Base URL`, `Model`, and `Temperature`.
- UI stack: Vue 3 + Naive UI.
- Package manager: pnpm.

## Agent tools

The agent can request these local tools while analyzing a report:

- `report_inventory`: report type, source, available data families and tool names.
- `overview`: TPS/MSPT/CPU/heap/entities/GC plus local threshold findings.
- `hotspots`: filtered CPU profile hotspots.
- `hotspot_groups`: CPU frames grouped by category/package/thread to reduce framework-frame noise.
- `hot_paths`: automatic child-frame drilldown. With `category:auto`, it selects high-percent actionable categories such as `entity_tick`, `chunk_task`, `commands`, `block_entity`, or `io`, then expands concrete classes/functions below aggregate frames like `EntityTickList`.
  It returns both flat `frames` and deep `callChains`; conclusions should prefer `callChains` because wrappers such as Neruina, Observable, and Mixin bridge frames are not usually the root cause.
- `mod_sources`: mod/source attribution using spark `class_sources`, `method_sources`, `line_sources`, and `metadata.sources`.
  It reports `resolvedSourceCount` and a verdict so the agent can distinguish "unknown dominates" from "all sources are unknown".
- `time_windows`: spark time-window statistics.
- `worst_windows`: worst windows by max MSPT, median MSPT, and low TPS, with previous/next-window deltas.
- `entities`: world/entity summaries and raw world statistics subset.
- `entity_chunks`: top entity-density chunks with entity mixes and local risk signals.
- `heap`: heap summary object ranking.
- `memory_gc`: heap, memory-pool, and GC aggregate statistics, including pause/frequency anomaly signals.
- `diagnostic_hypotheses`: local evidence synthesis that cross-checks hotspots, mod sources, entity chunks, and worst windows.
- `evidence_gaps`: what the current report can prove, cannot prove, and what to capture next.
- `raw_field`: bounded read of a raw protobuf field path.

The important design choice is that the AI is not trusted to invent precision from a single summary. It must call the tools first. When the report cannot uniquely prove the exact instance/chunk/mod, the agent is instructed to say that and name the follow-up capture, usually `/spark profiler --only-ticks-over 50 --timeout 120`.

## Run

```powershell
pnpm install
pnpm run dev
```

For frontend-only iteration:

```powershell
pnpm run dev:web
```

The local Vite URL is:

```text
http://127.0.0.1:1420
```

## Build

```powershell
pnpm run tauri build
```

Local Tauri build outputs use Tauri's default names, for example:

- `src-tauri\target\release\bro-know-my-spark-analyzer.exe`
- `src-tauri\target\release\bundle\msi\BroKnowMySparkAnalyzer_0.1.0_x64_en-US.msi`
- `src-tauri\target\release\bundle\nsis\BroKnowMySparkAnalyzer_0.1.0_x64-setup.exe`

The release `.exe` can be used as a portable Windows build as long as the machine has the required WebView2 runtime. The MSI/NSIS packages are optional installers. Tauri is cross-platform in principle, but desktop bundles should be built separately on Windows, macOS, and Linux for their native package formats.

## Release

GitHub Actions publishes releases from version tags:

```powershell
git tag v0.1.1
git push origin v0.1.1
```

The workflow parses the tag version, temporarily syncs `package.json`, `src-tauri/tauri.conf.json`, and `src-tauri/Cargo.toml`, builds each desktop platform, then renames assets to lowercase kebab-case.

Expected release asset names:

```text
bro-know-my-spark-analyzer-0.1.1-windows-x64-portable.exe
bro-know-my-spark-analyzer-0.1.1-windows-x64-setup.exe
bro-know-my-spark-analyzer-0.1.1-windows-x64.msi
bro-know-my-spark-analyzer-0.1.1-linux-x64.appimage
bro-know-my-spark-analyzer-0.1.1-linux-x64.deb
bro-know-my-spark-analyzer-0.1.1-linux-x64.rpm
bro-know-my-spark-analyzer-0.1.1-macos-x64.dmg
bro-know-my-spark-analyzer-0.1.1-macos-arm64.dmg
```

Current public builds are unsigned. Windows may show SmartScreen warnings, and macOS may require manual approval in Privacy & Security on first launch.

## AI provider presets

The first version treats providers as OpenAI-compatible chat-completions endpoints:

- OpenAI: `https://api.openai.com/v1`
- NewAPI Happy: `https://newapi.hello-happy.world/v1`
- DeepSeek: `https://api.deepseek.com/v1`
- Moonshot: `https://api.moonshot.cn/v1`
- SiliconFlow: `https://api.siliconflow.cn/v1`
- OpenRouter: `https://openrouter.ai/api/v1`
- Custom: user-supplied Base URL

Claude official API is intentionally not wired as a preset yet because it is not OpenAI-compatible. Add a second adapter when needed.

## Notes From Spark Source Review

Spark report upload/save paths are defined in `spark-common`:

- CPU profile upload content type: `application/x-spark-sampler`
- Heap summary upload content type: `application/x-spark-heap`
- Health upload content type: `application/x-spark-health`
- CPU profile fallback save: `config/spark/profile-<timestamp>.sparkprofile`
- Heap summary fallback save: `config/spark/heapsummary-<timestamp>.sparkheap`

The copied protobuf schema lives in `public/proto/`. The nested `public/proto/spark/` copy is kept for compatibility with spark's internal import paths.

## Local Fixture Testing

Use the inspection helpers with local `.sparkprofile` fixtures:

```powershell
node scripts\inspect-sparkprofile.mjs path\to\sample.sparkprofile
node scripts\agent-smoke.mjs path\to\sample.sparkprofile
```

`inspect-sparkprofile.mjs` also writes `<report>.prompt.txt` next to each input report for prompt inspection. `agent-smoke.mjs` exercises the report-tool layer used by the AI agent.

Next useful user action in game:

```text
/spark profiler --stop --save-to-file
```

or use whichever spark command variant exposes the save flag in the installed version. The output path should be under `config/spark/`.

## Verification Done

- `pnpm install`
- `pnpm run build`
- `cargo check` in `src-tauri`
- `pnpm run tauri build`
- Decoded local `.sparkprofile` fixtures with `protobufjs`.
- Verified `agent-smoke.mjs` against local fixtures.
- For TPS/MSPT conclusions, server-tick categories are scoped to `Server thread`. Background threads such as Netty, worker, or async sync threads are reported as background pressure unless explicitly tied back to Server thread evidence.
- CPU hotspot filtering removes Minecraft/DedicatedServer top-level loops, idle waits, Netty executor wrappers, FileWatcher, Lambda, and Mixin bridge frames from the default actionable hotspot list. Raw payloads remain available under Debug.
- `hotspot_groups` keeps `other` available but sorts it behind actionable categories such as `block_entity`, `entity_tick`, and `chunk_task`.
- `memory_gc` detects GC abnormalities even when heap usage is not near max.
- Reports with partial source maps must not be treated as all-unknown when `resolvedSourceCount` or notable source attribution is present.
