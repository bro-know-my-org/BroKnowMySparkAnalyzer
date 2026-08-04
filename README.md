# BroKnowMySparkAnalyzer (`bkmsa`)

[中文](#中文) | [English](#english)

## 中文

BroKnowMySparkAnalyzer 是 Minecraft [spark](https://spark.lucko.me/) 报告分析器。Rust 是唯一的解析、诊断和 AI agent 实现；Vue/TypeScript 只负责界面，Tauri 只负责桌面系统适配。

同一套 Rust 核心同时服务于：

- `bkmsa` 原生 CLI；
- Tauri 桌面应用；
- `bkmsa-wasm` WebAssembly 浏览器后端。

### 架构

```text
crates/bkmsa-core    protobuf 解码、摘要、确定性报告工具和诊断规则
crates/bkmsa-agent   基于证据工具的 OpenAI-compatible agent
crates/bkmsa-cli     原生 bkmsa 命令行程序
crates/bkmsa-wasm    core/agent 的浏览器 WASM 适配层
src-tauri            报告会话、网络、凭据和文件操作的 thin shell
packages/spark-analyzer
                     纯 Vue UI，通过 SparkAnalyzerAdapter 调用宿主
```

报告在 Rust/Tauri 或 WASM 内以 `reportId` 保存。UI 只接收摘要和工具结果，不解析 protobuf，也不包含诊断规则或 AI transport。

支持本地 `.sparkprofile`、`.sparkheap`、原始 health protobuf、文本日志、标准输入，以及 spark viewer/content URL 或报告 key。通过标准输入传文本时必须显式加 `--text`，避免损坏的 protobuf 被静默当成日志接受。

### CLI

开发时可通过 Cargo 或 pnpm 运行：

```bash
cargo run -p bkmsa-cli -- inspect report.sparkprofile
pnpm bkmsa -- inspect report.sparkprofile
```

构建原生二进制：

```bash
cargo build --release -p bkmsa-cli
./target/release/bkmsa --help
```

常用命令：

```bash
bkmsa tools --format terminal
bkmsa inventory report.sparkprofile --format json
bkmsa inspect report.sparkprofile
bkmsa inspect - --format json
bkmsa inspect - --text --format json # 从 stdin 读取文本日志
bkmsa tool report.sparkprofile overview
bkmsa tool report.sparkprofile hot-paths --category auto --limit 16
bkmsa tool report.sparkprofile raw-field --path metadata.platformStatistics
bkmsa analyze report.sparkprofile --format markdown --output diagnosis.md
bkmsa analyze https://spark.lucko.me/<key>
```

工具名可使用 `hot_paths` 或 `hot-paths`。复杂参数使用 `--args '{"limit":12}'`，简单参数也可重复传入 `--arg KEY=VALUE`。所有命令支持 `--format terminal|json|markdown` 和 `--output <path>`。

AI 分析配置：

```bash
export BKMSA_API_KEY="..."
export BKMSA_BASE_URL="https://api.openai.com/v1"
export BKMSA_MODEL="gpt-4.1-mini"
export BKMSA_TEMPERATURE="0.2"

bkmsa analyze report.sparkprofile --max-rounds 12
```

也可写入平台配置目录下的 `bkmsa/config.toml`，或通过 `--config` / `BKMSA_CONFIG` 指定文件：

```toml
base_url = "https://api.openai.com/v1"
model = "gpt-4.1-mini"
temperature = 0.2
```

配置优先级为命令行/环境变量、配置文件、内置默认值。API Key 可以写入配置文件，但更推荐使用 `BKMSA_API_KEY`，避免明文落盘。

`--api-key` 可用但不推荐，因为可能进入 shell history。CLI 退出码：

| 代码 | 含义 |
| ---: | --- |
| 0 | 成功 |
| 2 | 参数或配置错误 |
| 3 | 报告读取或下载失败 |
| 4 | protobuf 解码失败 |
| 5 | AI provider 失败 |
| 6 | 分析或输出失败 |

确定性工具包括 `overview`、`environment`、`hotspots`、`hotspot_groups`、`hot_paths`、`mod_sources`、`time_windows`、`worst_windows`、`entities`、`entity_chunks`、`heap`、`memory_gc`、`evidence_links`、`diagnostic_hypotheses`、`evidence_gaps` 和受限的 `raw_field`。以 `bkmsa tools` 输出为当前权威清单。

### 开发与构建

要求：稳定版 Rust、Node.js 22、pnpm 11；Web 构建还需要 `wasm-pack`，桌面构建还需要对应平台的 Tauri 2 系统依赖。

```bash
pnpm install
cargo test --workspace --all-targets
pnpm run build
```

桌面开发与打包：

```bash
pnpm run dev
pnpm run tauri build
```

Web/WASM 开发与构建：

```bash
pnpm run build:wasm
pnpm run test:wasm
pnpm run dev:web

# 静态发布构建
pnpm run build:wasm
pnpm run build:web
```

`build:wasm` 使用 `wasm-pack --target web` 将模块生成到 `public/bkmsa-wasm/`；Vite 将它与 Web UI 一起发布到 `dist/`。也可用 `VITE_BKMSA_WASM_MODULE` 指向外部 WASM JS 模块。远程 spark URL 若受浏览器 CORS 限制，可上传本地报告，或通过 `VITE_SPARK_PROXY_URL` 配置可信白名单代理。Web 端 AI provider 同样必须允许浏览器 CORS。

单独构建可复用 UI 包：

```bash
pnpm --dir packages/spark-analyzer build
```

### 测试策略

仓库不能依赖真实服务器报告或私有 `.sparkprofile` fixture。核心测试因此使用合成 protobuf/`Report` 数据验证解析、分类、证据与诊断契约；CLI 集成测试验证命令、JSON envelope 和退出码；agent 测试使用 mock provider，不调用真实 AI 服务。

私有真实报告只适合本地补充回归，不能提交到仓库。发现真实报告暴露的新结构时，应先最小化并匿名化为合成 fixture 或结构化契约测试，再修复 Rust 核心。旧的 `scripts/*.mjs` 只保留为 `bkmsa` CLI 兼容包装，不含第二套解析/分析逻辑。

### 发布资产

推送 `v*` tag（或手动触发 Release workflow）会为 Windows、Linux 和 macOS 构建桌面包及原生 CLI。版本 `0.1.1` 的预期资产示例：

```text
bkmsa-0.1.1-windows-x64.exe
bkmsa-0.1.1-linux-x64
bkmsa-0.1.1-macos-x64
bkmsa-0.1.1-macos-arm64

bro-know-my-spark-analyzer-0.1.1-windows-x64-portable.exe
bro-know-my-spark-analyzer-0.1.1-windows-x64-setup.exe
bro-know-my-spark-analyzer-0.1.1-windows-x64.msi
bro-know-my-spark-analyzer-0.1.1-linux-x64.appimage
bro-know-my-spark-analyzer-0.1.1-linux-x64.deb
bro-know-my-spark-analyzer-0.1.1-linux-x64.rpm
bro-know-my-spark-analyzer-0.1.1-macos-x64.dmg
bro-know-my-spark-analyzer-0.1.1-macos-arm64.dmg
```

macOS DMG 当前是可选产物；若 Tauri 未生成 DMG，release 仍可只包含该平台 CLI。当前公开桌面构建未签名。

## English

BroKnowMySparkAnalyzer analyzes Minecraft [spark](https://spark.lucko.me/) reports. Rust is the sole implementation of parsing, diagnostics, and the AI agent; Vue/TypeScript is UI only, while Tauri is a thin desktop adapter.

The same Rust core powers:

- the native `bkmsa` CLI;
- the Tauri desktop app;
- the `bkmsa-wasm` browser backend.

### Architecture

```text
crates/bkmsa-core    protobuf decoding, summaries, deterministic tools and rules
crates/bkmsa-agent   evidence-driven OpenAI-compatible agent
crates/bkmsa-cli     native bkmsa command-line program
crates/bkmsa-wasm    browser WASM adapter for core/agent
src-tauri            thin shell for sessions, network, credentials and files
packages/spark-analyzer
                     pure Vue UI using SparkAnalyzerAdapter
```

Reports remain behind a `reportId` inside Rust/Tauri or WASM. The UI receives summaries and tool results; it does not decode protobuf, implement diagnostic rules, or provide an AI transport.

Supported inputs include local `.sparkprofile`, `.sparkheap`, raw health protobuf, text logs, stdin, spark viewer/content URLs, and report keys. Text read from stdin must be selected explicitly with `--text`; this prevents damaged protobuf reports from being silently accepted as logs.

### CLI

Run from the workspace during development:

```bash
cargo run -p bkmsa-cli -- inspect report.sparkprofile
pnpm bkmsa -- inspect report.sparkprofile
```

Build the native executable:

```bash
cargo build --release -p bkmsa-cli
./target/release/bkmsa --help
```

Common commands:

```bash
bkmsa tools --format terminal
bkmsa inventory report.sparkprofile --format json
bkmsa inspect report.sparkprofile
bkmsa inspect - --format json
bkmsa inspect - --text --format json # read a text log from stdin
bkmsa tool report.sparkprofile overview
bkmsa tool report.sparkprofile hot-paths --category auto --limit 16
bkmsa tool report.sparkprofile raw-field --path metadata.platformStatistics
bkmsa analyze report.sparkprofile --format markdown --output diagnosis.md
bkmsa analyze https://spark.lucko.me/<key>
```

Tool names accept either `hot_paths` or `hot-paths`. Use `--args '{"limit":12}'` for a complete JSON object, or repeat `--arg KEY=VALUE` for individual arguments. Every command supports `--format terminal|json|markdown` and `--output <path>`.

AI configuration:

```bash
export BKMSA_API_KEY="..."
export BKMSA_BASE_URL="https://api.openai.com/v1"
export BKMSA_MODEL="gpt-4.1-mini"
export BKMSA_TEMPERATURE="0.2"

bkmsa analyze report.sparkprofile --max-rounds 12
```

Configuration may also live in `bkmsa/config.toml` below the platform config directory, or in a file selected through `--config` / `BKMSA_CONFIG`:

```toml
base_url = "https://api.openai.com/v1"
model = "gpt-4.1-mini"
temperature = 0.2
```

Precedence is command line/environment, configuration file, then built-in defaults. A key may be stored in TOML, but `BKMSA_API_KEY` is recommended to avoid a plaintext secret on disk.

`--api-key` is available but discouraged because it may be stored in shell history. CLI exit codes are:

| Code | Meaning |
| ---: | --- |
| 0 | Success |
| 2 | Argument or configuration error |
| 3 | Report read or download failure |
| 4 | Protobuf decode failure |
| 5 | AI provider failure |
| 6 | Analysis or output failure |

Deterministic tools include `overview`, `environment`, `hotspots`, `hotspot_groups`, `hot_paths`, `mod_sources`, `time_windows`, `worst_windows`, `entities`, `entity_chunks`, `heap`, `memory_gc`, `evidence_links`, `diagnostic_hypotheses`, `evidence_gaps`, and bounded `raw_field`. Treat `bkmsa tools` as the authoritative current list.

### Development and builds

Requirements: stable Rust, Node.js 22, and pnpm 11. Web builds additionally need `wasm-pack`; desktop builds need the Tauri 2 system dependencies for the target platform.

```bash
pnpm install
cargo test --workspace --all-targets
pnpm run build
```

Desktop development and packaging:

```bash
pnpm run dev
pnpm run tauri build
```

Web/WASM development and production builds:

```bash
pnpm run build:wasm
pnpm run test:wasm
pnpm run dev:web

# static production build
pnpm run build:wasm
pnpm run build:web
```

`build:wasm` runs `wasm-pack --target web` and writes the module to `public/bkmsa-wasm/`; Vite publishes it with the Web UI in `dist/`. `VITE_BKMSA_WASM_MODULE` may point to an external WASM JS module instead. If browser CORS blocks remote spark URLs, upload a local report or configure a trusted allow-listed proxy through `VITE_SPARK_PROXY_URL`. Web AI providers must also allow browser CORS.

Build the reusable UI package separately with:

```bash
pnpm --dir packages/spark-analyzer build
```

### Test strategy

The repository must not depend on real server reports or private `.sparkprofile` fixtures. Core tests therefore use synthetic protobuf/`Report` data to verify parsing, classification, evidence, and diagnostic contracts. CLI integration tests verify commands, JSON envelopes, and exit codes. Agent tests use mock providers and never call a real AI service.

Private real-world reports are optional local regression inputs and must not be committed. When one exposes a new structure, minimize and anonymize it into a synthetic fixture or structured contract test before changing the Rust core. Legacy `scripts/*.mjs` files are compatibility wrappers around `bkmsa`; they contain no second parser or analyzer implementation.

### Release assets

Pushing a `v*` tag (or manually dispatching the Release workflow) builds desktop packages and native CLIs for Windows, Linux, and macOS. Expected assets for version `0.1.1` include:

```text
bkmsa-0.1.1-windows-x64.exe
bkmsa-0.1.1-linux-x64
bkmsa-0.1.1-macos-x64
bkmsa-0.1.1-macos-arm64

bro-know-my-spark-analyzer-0.1.1-windows-x64-portable.exe
bro-know-my-spark-analyzer-0.1.1-windows-x64-setup.exe
bro-know-my-spark-analyzer-0.1.1-windows-x64.msi
bro-know-my-spark-analyzer-0.1.1-linux-x64.appimage
bro-know-my-spark-analyzer-0.1.1-linux-x64.deb
bro-know-my-spark-analyzer-0.1.1-linux-x64.rpm
bro-know-my-spark-analyzer-0.1.1-macos-x64.dmg
bro-know-my-spark-analyzer-0.1.1-macos-arm64.dmg
```

macOS DMGs are currently optional; a release may contain only the CLI for that macOS target when Tauri does not produce a DMG. Public desktop builds are currently unsigned.
