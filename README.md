# BroKnowMySparkAnalyzer

[中文](#中文) | [English](#english)

## 中文

BroKnowMySparkAnalyzer 是一个用于本地检查 Minecraft spark 性能报告的 Tauri 桌面应用。AI 部分以小型报告分析 agent 的形式运行：它会查看可用的本地报告工具，按需请求证据，然后基于已收集的数据输出 Markdown 诊断。

### 当前范围

- 导入本地 spark protobuf 文件：
  - CPU profiler fallback 保存的 `.sparkprofile`。
  - heap summary fallback 保存的 `.sparkheap`。
  - 如果单独保存，也可以导入原始 health protobuf 文件。
- 粘贴 `spark.lucko.me` viewer URL、`spark-usercontent.lucko.me` URL 或原始 bytebin key。
- 粘贴纯文本日志或手动复制的报告笔记。
- 使用从 `lucko/spark` 复制的 schema 解码 protobuf。
- 生成 TPS、MSPT、CPU、heap、entities、GC、CPU hotspots 和 heap object hotspots 的本地发现。
- 使用 AI 工具循环分析报告，而不是把所有数据塞进单个手写 prompt。
- 将最终诊断渲染为 Markdown，并显示 agent 工具调用轨迹。
- 默认中文和深色模式，顶部栏提供语言切换和明暗主题切换。
- 原始 agent/tool payload 放在 Debug 开关后面；普通模式只展示简洁的工具调用摘要。
- 诊断面板支持全屏查看、Markdown 导出和 PNG 图片导出。
- 主面板和指标卡片可以通过浏览器 resize handle 手动纵向调整高度。
- 设置面板支持测试 AI provider 连通性。
- 支持从 OpenAI-compatible `/models` 获取模型列表；不暴露模型列表的 provider 仍可手动编辑模型名。
- 温度等不常用 AI 参数收在 Advanced AI Settings 折叠区域下，默认 temperature 为 `0.2`。
- 远程报告抓取时，抓取按钮会显示 loading 状态。
- 生成诊断后，追问面板可以基于当前报告、最终诊断和已收集工具证据继续向 AI 提问。
- 通过可配置的 `API Key`、`Base URL`、`Model` 和 `Temperature` 调用 OpenAI-compatible `/chat/completions` provider。
- UI 技术栈：Vue 3 + Naive UI。
- 包管理器：pnpm。

### Agent 工具

分析报告时，agent 可以请求这些本地工具：

- `report_inventory`：报告类型、来源、可用数据族和工具名。
- `overview`：TPS/MSPT/CPU/heap/entities/GC，以及本地阈值发现。
- `hotspots`：过滤后的 CPU profile hotspots。
- `hotspot_groups`：按类别、包名和线程分组 CPU frames，减少框架层 frame 噪声。
- `hot_paths`：自动子帧下钻。使用 `category:auto` 时，会选择 `entity_tick`、`chunk_task`、`commands`、`block_entity` 或 `io` 等高占比可行动类别，再展开聚合 frame 下面的具体类/函数。
  它同时返回扁平 `frames` 和深层 `callChains`；结论应优先使用 `callChains`，因为包装层 frame 通常不是根因。
- `mod_sources`：使用 spark `class_sources`、`method_sources`、`line_sources` 和 `metadata.sources` 做 mod/source 归因。
  它会报告 `resolvedSourceCount` 和 verdict，帮助 agent 区分 “unknown 占主导” 和 “全部都是 unknown”。
- `time_windows`：spark 时间窗口统计。
- `worst_windows`：按最大 MSPT、median MSPT 和低 TPS 排序的最差窗口，并带前后窗口 delta。
- `entities`：world/entity 摘要和原始 world statistics 子集。
- `entity_chunks`：实体密度最高的 chunk、实体混合和本地风险信号。
- `heap`：heap summary 对象排行。
- `memory_gc`：heap、memory-pool 和 GC 聚合统计，包括 pause/frequency 异常信号。
- `diagnostic_hypotheses`：本地证据综合，交叉检查 hotspots、mod sources、entity chunks 和 worst windows。
- `evidence_gaps`：当前报告能证明什么、不能证明什么，以及下一步需要采集什么。
- `raw_field`：有边界地读取原始 protobuf 字段路径。

核心设计选择是：AI 不能只靠单个摘要编造精确结论。它必须先调用工具。当前报告无法唯一证明具体实例、chunk 或 mod 时，agent 必须明确说明证据不足，并给出下一步采集方式，通常是 `/spark profiler --only-ticks-over 50 --timeout 120`。

### 运行

```powershell
pnpm install
pnpm run dev
```

仅做前端迭代：

```powershell
pnpm run dev:web
```

本地 Vite 地址：

```text
http://127.0.0.1:1420
```

纯 Web 版本与桌面版共用同一套分析界面和报告逻辑。浏览器中可直接上传并本地解析 spark 报告、运行可视化分析、导出 Markdown/PNG，并在 AI provider 支持 CORS 时直接调用 AI。构建静态站点：

```powershell
pnpm run build:web
```

产物位于 `dist`，可以部署到 GitHub Pages、Cloudflare Pages、Vercel 或任意静态文件服务。远程 spark 链接若被浏览器 CORS 拦截，可改用本地上传，或在构建时配置 `VITE_SPARK_PROXY_URL` 指向受信任的白名单代理。

### 构建

```powershell
pnpm run tauri build
```

本地 Tauri 构建产物使用 Tauri 默认命名，例如：

- `src-tauri\target\release\bro-know-my-spark-analyzer.exe`
- `src-tauri\target\release\bundle\msi\BroKnowMySparkAnalyzer_0.1.0_x64_en-US.msi`
- `src-tauri\target\release\bundle\nsis\BroKnowMySparkAnalyzer_0.1.0_x64-setup.exe`

release `.exe` 可以作为 Windows portable build 使用，前提是目标机器安装了所需的 WebView2 runtime。MSI/NSIS 是可选安装包。Tauri 原则上跨平台，但桌面 bundle 应分别在 Windows、macOS 和 Linux 上为各自原生包格式构建。

### 发布

GitHub Actions 会从版本 tag 发布 release：

```powershell
git tag v0.1.1
git push origin v0.1.1
```

workflow 会解析 tag 版本，临时同步 `package.json`、`src-tauri/tauri.conf.json` 和 `src-tauri/Cargo.toml`，在各桌面平台构建，然后把资产重命名为小写 kebab-case。

预期 release asset 名称：

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

当前公开构建未签名。Windows 可能显示 SmartScreen 提示，macOS 首次启动可能需要在 Privacy & Security 中手动允许。

### AI Provider Presets

当前版本把 provider 视为 OpenAI-compatible chat-completions endpoint：

- OpenAI：`https://api.openai.com/v1`
- NewAPI Happy：`https://newapi.hello-happy.world/v1`
- DeepSeek：`https://api.deepseek.com/v1`
- Moonshot：`https://api.moonshot.cn/v1`
- SiliconFlow：`https://api.siliconflow.cn/v1`
- OpenRouter：`https://openrouter.ai/api/v1`
- Custom：用户自定义 Base URL

Claude official API 目前没有作为 preset 接入，因为它不是 OpenAI-compatible。需要时可以添加第二个 adapter。

### Spark Source Review Notes

spark 报告上传/保存路径定义在 `spark-common`：

- CPU profile upload content type：`application/x-spark-sampler`
- Heap summary upload content type：`application/x-spark-heap`
- Health upload content type：`application/x-spark-health`
- CPU profile fallback save：`config/spark/profile-<timestamp>.sparkprofile`
- Heap summary fallback save：`config/spark/heapsummary-<timestamp>.sparkheap`

复制的 protobuf schema 位于 `public/proto/`。嵌套的 `public/proto/spark/` 副本用于兼容 spark 内部 import path。

### 本地 Fixture 测试

使用检查脚本分析本地 `.sparkprofile` fixture：

```powershell
node scripts\inspect-sparkprofile.mjs path\to\sample.sparkprofile
node scripts\agent-smoke.mjs path\to\sample.sparkprofile
```

`inspect-sparkprofile.mjs` 也会在每个输入报告旁边写入 `<report>.prompt.txt`，方便检查 prompt。`agent-smoke.mjs` 用于测试 AI agent 使用的 report-tool 层。

游戏内下一步常用采集命令：

```text
/spark profiler --stop --save-to-file
```

也可以使用当前 spark 版本提供的保存 flag。输出路径通常在 `config/spark/` 下。

### 已验证

- `pnpm install`
- `pnpm run build`
- `cargo check` in `src-tauri`
- `pnpm run tauri build`
- 使用 `protobufjs` 解码本地 `.sparkprofile` fixtures。
- 使用本地 fixtures 验证 `agent-smoke.mjs`。
- TPS/MSPT 结论中，server-tick 类别只按 `Server thread` 归因。Netty、worker 或 async sync 等后台线程只作为后台压力说明，除非证据明确关联回 Server thread。
- CPU hotspot 过滤会从默认可行动 hotspot 列表中移除 Minecraft/DedicatedServer 顶层循环、idle waits、Netty executor wrappers、FileWatcher、Lambda 和 Mixin bridge frames。原始 payload 仍可在 Debug 中查看。
- `hotspot_groups` 保留 `other`，但会排在 `block_entity`、`entity_tick` 和 `chunk_task` 等可行动类别后面。
- `memory_gc` 可以在 heap usage 未接近上限时识别 GC 异常。
- 如果 `resolvedSourceCount` 或 notable source attribution 存在，部分 source maps 报告不能被当成全 unknown。

## English

BroKnowMySparkAnalyzer is a Tauri desktop app for inspecting Minecraft spark performance reports locally. The AI side runs as a small report-analysis agent: it sees the available local report tools, requests the evidence it needs, and then writes a Markdown diagnosis from the collected data.

### Current Scope

- Import local spark protobuf files:
  - `.sparkprofile` from CPU profiler fallback saves.
  - `.sparkheap` from heap summary fallback saves.
  - raw health protobuf files if saved separately.
- Paste a `spark.lucko.me` viewer URL, `spark-usercontent.lucko.me` URL, or raw bytebin key.
- Paste plain text logs or manually copied report notes.
- Decode protobuf with the schema copied from `lucko/spark`.
- Generate local findings for TPS, MSPT, CPU, heap, entities, GC, CPU hotspots, and heap object hotspots.
- Run an AI tool loop over the report instead of sending one large hand-built prompt.
- Render the final diagnosis as Markdown and show the agent tool trace.
- Default to Chinese and dark mode, with top-bar language and light/dark theme switches.
- Keep raw agent/tool payloads behind a Debug switch. Normal mode shows concise tool-call summaries.
- Support fullscreen diagnosis viewing, Markdown export, and PNG image export.
- Allow main panels and metric cards to be resized vertically from the browser resize handle.
- Test AI provider connectivity from the settings panel.
- Fetch OpenAI-compatible model lists from `/models`; the model selector remains manually editable for providers that do not expose model listing.
- Keep less-used AI knobs such as temperature under a collapsed Advanced AI Settings section. Default temperature is `0.2`.
- Show a loading state while fetching remote reports.
- After a diagnosis is generated, ask follow-up questions using the current report, final diagnosis, and collected tool evidence as context.
- Call OpenAI-compatible `/chat/completions` providers with configurable `API Key`, `Base URL`, `Model`, and `Temperature`.
- UI stack: Vue 3 + Naive UI.
- Package manager: pnpm.

### Agent Tools

The agent can request these local tools while analyzing a report:

- `report_inventory`: report type, source, available data families and tool names.
- `overview`: TPS/MSPT/CPU/heap/entities/GC plus local threshold findings.
- `hotspots`: filtered CPU profile hotspots.
- `hotspot_groups`: CPU frames grouped by category/package/thread to reduce framework-frame noise.
- `hot_paths`: automatic child-frame drilldown. With `category:auto`, it selects high-percent actionable categories such as `entity_tick`, `chunk_task`, `commands`, `block_entity`, or `io`, then expands concrete classes/functions below aggregate frames.
  It returns both flat `frames` and deep `callChains`; conclusions should prefer `callChains` because wrapper frames are usually not the root cause.
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

The important design choice is that the AI is not trusted to invent precision from a single summary. It must call the tools first. When the report cannot uniquely prove the exact instance, chunk, or mod, the agent is instructed to say so and name the follow-up capture, usually `/spark profiler --only-ticks-over 50 --timeout 120`.

### Run

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

The pure Web build shares the analyzer UI and report logic with the desktop app. It can parse local spark reports, render visual summaries, export Markdown/PNG, and call AI providers directly when they allow CORS:

```powershell
pnpm run build:web
```

Deploy the generated `dist` directory to GitHub Pages, Cloudflare Pages, Vercel, or any static host. If browser CORS blocks remote spark URLs, use local upload or set `VITE_SPARK_PROXY_URL` at build time to a trusted allow-listed proxy.

### Build

```powershell
pnpm run tauri build
```

Local Tauri build outputs use Tauri's default names, for example:

- `src-tauri\target\release\bro-know-my-spark-analyzer.exe`
- `src-tauri\target\release\bundle\msi\BroKnowMySparkAnalyzer_0.1.0_x64_en-US.msi`
- `src-tauri\target\release\bundle\nsis\BroKnowMySparkAnalyzer_0.1.0_x64-setup.exe`

The release `.exe` can be used as a portable Windows build as long as the machine has the required WebView2 runtime. The MSI/NSIS packages are optional installers. Tauri is cross-platform in principle, but desktop bundles should be built separately on Windows, macOS, and Linux for their native package formats.

### Release

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

### AI Provider Presets

The current version treats providers as OpenAI-compatible chat-completions endpoints:

- OpenAI: `https://api.openai.com/v1`
- NewAPI Happy: `https://newapi.hello-happy.world/v1`
- DeepSeek: `https://api.deepseek.com/v1`
- Moonshot: `https://api.moonshot.cn/v1`
- SiliconFlow: `https://api.siliconflow.cn/v1`
- OpenRouter: `https://openrouter.ai/api/v1`
- Custom: user-supplied Base URL

Claude official API is intentionally not wired as a preset yet because it is not OpenAI-compatible. Add a second adapter when needed.

### Spark Source Review Notes

Spark report upload/save paths are defined in `spark-common`:

- CPU profile upload content type: `application/x-spark-sampler`
- Heap summary upload content type: `application/x-spark-heap`
- Health upload content type: `application/x-spark-health`
- CPU profile fallback save: `config/spark/profile-<timestamp>.sparkprofile`
- Heap summary fallback save: `config/spark/heapsummary-<timestamp>.sparkheap`

The copied protobuf schema lives in `public/proto/`. The nested `public/proto/spark/` copy is kept for compatibility with spark's internal import paths.

### Local Fixture Testing

Use the inspection helpers with local `.sparkprofile` fixtures:

```powershell
node scripts\inspect-sparkprofile.mjs path\to\sample.sparkprofile
node scripts\agent-smoke.mjs path\to\sample.sparkprofile
```

`inspect-sparkprofile.mjs` also writes `<report>.prompt.txt` next to each input report for prompt inspection. `agent-smoke.mjs` exercises the report-tool layer used by the AI agent.

Next useful in-game capture command:

```text
/spark profiler --stop --save-to-file
```

Or use whichever spark command variant exposes the save flag in the installed version. The output path should be under `config/spark/`.

### Verification Done

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
