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
crates/bkmsa-tauri   可复用的 Tauri 2 原生分析插件
crates/bkmsa-cli     原生 bkmsa 命令行程序
crates/bkmsa-wasm    core/agent 的浏览器 WASM 适配层
src-tauri            注册 bkmsa-tauri 的独立桌面应用壳
packages/spark-analyzer
                     Vue UI，以及可选的 Tauri adapter
```

报告在 Rust/Tauri 或 WASM 内以 `reportId` 保存。UI 只接收摘要和工具结果，不解析 protobuf，也不包含诊断规则或 AI transport。

独立 Analyzer 与 BroKnowMyToolbox 都注册同一个 `bkmsa-tauri` 插件，并使用 npm 包的 `createTauriSparkAnalyzerAdapter()`。因此两个应用共享完全相同的解析、工具、Agent、请求限制和取消语义；应用自身只负责窗口和工具布局。

支持本地 `.sparkprofile`、`.sparkheap`、原始 health protobuf、文本日志、标准输入，以及 spark viewer/content URL 或报告 key。通过标准输入传文本时必须显式加 `--text`，避免损坏的 protobuf 被静默当成日志接受。

### 选择使用方式

| 方式 | 适合场景 | 如何使用 |
| --- | --- | --- |
| Web | 不安装应用，直接在浏览器分析本地报告 | 打开 [GitHub Pages](https://bro-know-my-org.github.io/BroKnowMySparkAnalyzer/)，或本地运行 `pnpm run dev:web` |
| 桌面 App | 需要原生文件选择、系统凭据存储和完整桌面体验 | 从 GitHub Release 下载对应平台安装包/便携包 |
| CLI | 服务器、脚本、批处理、CI 或终端使用 | `cargo install bkmsa-cli`，安装后的命令是 `bkmsa` |
| npm UI 包 | 把同一套分析器界面嵌入 Vue/Tauri 应用 | 安装 `@bro-know-my/spark-analyzer` 并提供 Tauri 或 WASM adapter |
| Rust crates | 在 Rust/Tauri 项目里直接复用核心、Agent 或原生插件 | 使用 `bkmsa-core`、`bkmsa-agent`、`bkmsa-tauri` |

### Web

直接使用线上 Web 版：

```text
https://bro-know-my-org.github.io/BroKnowMySparkAnalyzer/
```

Web 版在浏览器内加载 `bkmsa-wasm`，本地报告不会为了完成确定性分析而上传到本项目服务器。远程 spark URL 和 AI provider 仍受浏览器 CORS 限制；遇到限制时可上传本地报告、配置可信代理，或改用桌面 App/CLI。

本地运行 Web 版：

```bash
pnpm install
pnpm run build:wasm
pnpm run dev:web
```

### 桌面 App

普通用户从 GitHub Release 下载 Windows、Linux 或 macOS 产物即可。桌面版使用 Rust/Tauri 后端，支持原生保存对话框、系统凭据存储、远程报告下载和请求取消。

从源码运行或打包：

```bash
pnpm install
pnpm run dev
pnpm run tauri build
```

### CLI

发布到 crates.io 后可一键安装；crate 名是 `bkmsa-cli`，可执行命令名是 `bkmsa`：

```bash
cargo install bkmsa-cli
bkmsa --help
```

也可以从 GitHub 或当前源码树安装：

```bash
cargo install --git https://github.com/bro-know-my-org/BroKnowMySparkAnalyzer bkmsa-cli
cargo install --path crates/bkmsa-cli
```

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

### 嵌入其他应用

Vue/Tauri 宿主安装 UI 包：

```bash
pnpm add @bro-know-my/spark-analyzer
pnpm add vue naive-ui @vicons/fa @vicons/fluent
```

纯 npm 包只包含 Vue UI，不包含另一套 TypeScript 分析逻辑。完整能力必须连接 `bkmsa-tauri` 或 `bkmsa-wasm` adapter；Tauri 集成示例见下方，完整自定义 adapter 接口见 [`packages/spark-analyzer/README.md`](packages/spark-analyzer/README.md)。

Rust 项目可按需要选择：

```bash
cargo add bkmsa-core
cargo add bkmsa-agent
cargo add bkmsa-tauri
```

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

在另一个 Tauri 2 应用中嵌入分析器：

```toml
[dependencies]
bkmsa-tauri = "0.1.1"
```

```rust
tauri::Builder::default()
    .plugin(bkmsa_tauri::init())
```

应用 capability 需要加入 `"bkmsa-tauri:default"`。前端安装 UI 和宿主插件后创建 adapter：

```ts
import { SparkAnalyzerView } from "@bro-know-my/spark-analyzer";
import { createTauriSparkAnalyzerAdapter } from "@bro-know-my/spark-analyzer/tauri";

const adapter = createTauriSparkAnalyzerAdapter();
```

### 测试策略

仓库不能依赖真实服务器报告或私有 `.sparkprofile` fixture。核心测试因此使用合成 protobuf/`Report` 数据验证解析、分类、证据与诊断契约；CLI 集成测试验证命令、JSON envelope 和退出码；agent 测试使用 mock provider，不调用真实 AI 服务。

私有真实报告只适合本地补充回归，不能提交到仓库。发现真实报告暴露的新结构时，应先最小化并匿名化为合成 fixture 或结构化契约测试，再修复 Rust 核心。旧的 `scripts/*.mjs` 只保留为 `bkmsa` CLI 兼容包装，不含第二套解析/分析逻辑。

### 发布资产

推送 tag 前必须先把 workspace 版本更新为目标版本。`Publish SDK Packages` workflow 会按依赖顺序发布 `bkmsa-core`、`bkmsa-agent`、`bkmsa-tauri`、`bkmsa-cli`，然后发布同版本的 `@bro-know-my/spark-analyzer`。仓库 Actions secrets 需要：

```text
CARGO_REGISTRY_TOKEN  crates.io 发布 token
NPM_TOKEN             npm 发布 token
```

首次 `cargo publish` 会自动创建 crate，不需要在 crates.io 手动建包。首次发布后可以把 token 收紧到这四个 crate，或迁移到 crates.io Trusted Publishing。crate 版本不可覆盖；workflow 支持安全重跑并跳过已经存在的同版本包。

推送到 `master` 会在 WASM smoke test 和 Web 构建成功后更新 GitHub Pages。推送严格的 `vX.Y.Z` tag（或手动触发 Release workflow）会先校验已提交版本并运行完整 CI，再构建 Windows、Linux 和 macOS 桌面包及原生 CLI；随后发布 Rust/npm SDK、部署网页并创建 GitHub Release。版本 `0.1.1` 的预期资产示例：

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
crates/bkmsa-tauri   reusable native Tauri 2 analyzer plugin
crates/bkmsa-cli     native bkmsa command-line program
crates/bkmsa-wasm    browser WASM adapter for core/agent
src-tauri            standalone desktop host that registers bkmsa-tauri
packages/spark-analyzer
                     Vue UI plus an optional Tauri adapter
```

Reports remain behind a `reportId` inside Rust/Tauri or WASM. The UI receives summaries and tool results; it does not decode protobuf, implement diagnostic rules, or provide an AI transport.

The standalone Analyzer and BroKnowMyToolbox both register the same `bkmsa-tauri` plugin and use `createTauriSparkAnalyzerAdapter()` from the npm package. Both applications therefore share identical parsing, tools, agent behavior, request limits, and cancellation semantics; each app only owns its window and surrounding layout.

Supported inputs include local `.sparkprofile`, `.sparkheap`, raw health protobuf, text logs, stdin, spark viewer/content URLs, and report keys. Text read from stdin must be selected explicitly with `--text`; this prevents damaged protobuf reports from being silently accepted as logs.

### Choose a distribution

| Distribution | Best for | How to use it |
| --- | --- | --- |
| Web | Analyze local reports in a browser without installing an app | Open [GitHub Pages](https://bro-know-my-org.github.io/BroKnowMySparkAnalyzer/) or run `pnpm run dev:web` locally |
| Desktop app | Native file dialogs, credential storage, and the complete desktop experience | Download the package/portable binary for your platform from GitHub Releases |
| CLI | Servers, scripts, batch jobs, CI, and terminal workflows | Run `cargo install bkmsa-cli`; the installed command is `bkmsa` |
| npm UI package | Embed the same analyzer UI in a Vue/Tauri application | Install `@bro-know-my/spark-analyzer` and provide a Tauri or WASM adapter |
| Rust crates | Reuse the core, agent, or native plugin from Rust/Tauri | Use `bkmsa-core`, `bkmsa-agent`, or `bkmsa-tauri` |

### Web

Use the hosted Web application at:

```text
https://bro-know-my-org.github.io/BroKnowMySparkAnalyzer/
```

The Web build loads `bkmsa-wasm` in the browser. Local reports do not need to be uploaded to a project server for deterministic analysis. Remote spark URLs and AI providers are still subject to browser CORS; upload a local report, configure a trusted proxy, or use the desktop app/CLI when necessary.

Run the Web application locally with:

```bash
pnpm install
pnpm run build:wasm
pnpm run dev:web
```

### Desktop app

Most users should download a Windows, Linux, or macOS artifact from GitHub Releases. The desktop build uses the Rust/Tauri backend and supports native save dialogs, system credential storage, remote report downloads, and request cancellation.

Run or package it from source with:

```bash
pnpm install
pnpm run dev
pnpm run tauri build
```

### CLI

Once published to crates.io, install it in one command. The crate is named `bkmsa-cli`; the executable is `bkmsa`:

```bash
cargo install bkmsa-cli
bkmsa --help
```

You may also install directly from GitHub or the current source tree:

```bash
cargo install --git https://github.com/bro-know-my-org/BroKnowMySparkAnalyzer bkmsa-cli
cargo install --path crates/bkmsa-cli
```

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

### Embedding in another application

Install the Vue/Tauri UI package with:

```bash
pnpm add @bro-know-my/spark-analyzer
pnpm add vue naive-ui @vicons/fa @vicons/fluent
```

The npm package is UI-only and does not contain a second TypeScript analyzer. Connect it to a `bkmsa-tauri` or `bkmsa-wasm` adapter for complete functionality. See the Tauri example below and [`packages/spark-analyzer/README.md`](packages/spark-analyzer/README.md) for the full custom adapter contract.

Rust consumers may select only the layer they need:

```bash
cargo add bkmsa-core
cargo add bkmsa-agent
cargo add bkmsa-tauri
```

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

To embed the analyzer in another Tauri 2 application:

```toml
[dependencies]
bkmsa-tauri = "0.1.1"
```

```rust
tauri::Builder::default()
    .plugin(bkmsa_tauri::init())
```

Add `"bkmsa-tauri:default"` to the application capability. Install the UI and host plugins, then create the frontend adapter:

```ts
import { SparkAnalyzerView } from "@bro-know-my/spark-analyzer";
import { createTauriSparkAnalyzerAdapter } from "@bro-know-my/spark-analyzer/tauri";

const adapter = createTauriSparkAnalyzerAdapter();
```

### Test strategy

The repository must not depend on real server reports or private `.sparkprofile` fixtures. Core tests therefore use synthetic protobuf/`Report` data to verify parsing, classification, evidence, and diagnostic contracts. CLI integration tests verify commands, JSON envelopes, and exit codes. Agent tests use mock providers and never call a real AI service.

Private real-world reports are optional local regression inputs and must not be committed. When one exposes a new structure, minimize and anonymize it into a synthetic fixture or structured contract test before changing the Rust core. Legacy `scripts/*.mjs` files are compatibility wrappers around `bkmsa`; they contain no second parser or analyzer implementation.

### Release assets

Before pushing a tag, update the workspace version to the intended release. The `Publish SDK Packages` workflow publishes `bkmsa-core`, `bkmsa-agent`, `bkmsa-tauri`, and `bkmsa-cli` in dependency order, followed by the matching `@bro-know-my/spark-analyzer` npm version. Configure these Actions secrets:

```text
CARGO_REGISTRY_TOKEN  crates.io publishing token
NPM_TOKEN             npm publishing token
```

The first `cargo publish` creates each crate automatically; no manual crates.io package creation is needed. After bootstrapping, restrict the token to these four crates or migrate to crates.io Trusted Publishing. Published versions cannot be overwritten. The workflow is rerunnable and skips matching versions that already exist.

Pushing to `master` updates GitHub Pages after the WASM smoke test and Web build succeed. Pushing a strict `vX.Y.Z` tag (or manually dispatching the Release workflow) first validates committed versions and runs the full CI gate, then builds desktop packages and native CLIs for Windows, Linux, and macOS. Rust/npm SDK publication, Web deployment, and the GitHub Release follow only after those gates pass. Expected assets for version `0.1.1` include:

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
