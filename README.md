# BroKnowMySparkAnalyzer

[中文](#中文) · [English](#english)

## 中文

Minecraft [spark](https://spark.lucko.me/) 报告分析工具，提供网页版、桌面版和命令行。导入报告后，查看 CPU 热点、卡顿时段、实体与区块统计、内存及 GC 数据；也可以接入 AI，围绕报告中的问题继续追问。

**[打开网页版](https://bro-know-my-org.github.io/BroKnowMySparkAnalyzer/)** · **[下载桌面版](https://github.com/bro-know-my-org/BroKnowMySparkAnalyzer/releases)**

### 能分析什么

| 排查方向 | 可查看的数据 |
| --- | --- |
| CPU 与卡顿 | 热点调用路径、耗时分布、卡顿时段 |
| 实体与区块 | 实体分布、区块统计 |
| 内存与 GC | 内存使用、GC 情况、堆内存对象排行 |
| AI 辅助排查 | 基于报告数据的分析建议，以及后续追问 |

具体可用的数据取决于导入的报告类型和采集内容。

### 开始使用

**1. 选择使用方式**

| 入口 | 适用场景 |
| --- | --- |
| [网页版](https://bro-know-my-org.github.io/BroKnowMySparkAnalyzer/) | 无需安装，在浏览器中导入和分析报告 |
| [桌面版](https://github.com/bro-know-my-org/BroKnowMySparkAnalyzer/releases) | 使用系统凭据存储和原生保存对话框 |
| `bkmsa` 命令行 | 在服务器上分析报告，或接入脚本、批处理 |

**2. 导入报告**

从 spark 导出文件，或复制报告链接 / key。文件导入支持 `.sparkprofile`、`.sparkheap`、`.sparkhealth` 和原始 health protobuf。文本日志请粘贴到网页版或桌面版的文本输入框。导入后即可查看报告数据，无需配置 AI 服务。

**3. 按需启用 AI**

在设置中选择或配置支持 OpenAI 兼容接口的服务，填入 API Key，并确认服务地址和模型名称。配置后即可发起分析或继续追问。

基础报告分析在本地完成，网页版也在浏览器内解析文件。使用 AI 时，分析所需的数据会发送给你配置的服务。

### 命令行

从 [Releases](https://github.com/bro-know-my-org/BroKnowMySparkAnalyzer/releases) 下载对应平台的 `bkmsa`，或通过 Rust 安装：

```bash
cargo install bkmsa-cli

# 查看报告摘要
bkmsa inspect report.sparkprofile

# 查看热点调用路径
bkmsa tool report.sparkprofile hot-paths --category auto --limit 16

# 导出 JSON，供其他程序处理
bkmsa inventory report.sparkprofile --format json
```

以上分析命令不需要 API Key。AI 分析、标准输入、配置文件和完整参数见 [CLI 文档](docs/CLI.md)。

### 使用提示

- 网页版导入远程 spark 链接或调用 AI 服务时，可能受到浏览器 CORS 限制。链接导入失败时，可先下载报告再导入文件；AI 请求被拦截时，可改用桌面版或 CLI。
- 桌面安装包目前未签名，macOS DMG 是否提供以对应 Release 为准。

### 更多文档

- [命令行](docs/CLI.md)：AI 配置、报告工具、JSON 导出和标准输入
- [本地开发](docs/DEVELOPMENT.md)：环境准备、运行、构建和检查
- [项目集成](docs/INTEGRATION.md)：Vue 界面、Rust crates 和 Tauri 插件
- [维护与发布](docs/MAINTAINING.md)：测试约定、SDK 发布和发版恢复

## English

A Minecraft [spark](https://spark.lucko.me/) report analyzer for the web, desktop, and command line. Import a report to explore CPU hotspots, slow periods, entity and chunk statistics, memory usage, and GC. Connect an AI provider to discuss findings and ask follow-up questions.

**[Open the web app](https://bro-know-my-org.github.io/BroKnowMySparkAnalyzer/)** · **[Download the desktop app](https://github.com/bro-know-my-org/BroKnowMySparkAnalyzer/releases)**

### What you can analyze

| Area | Available data |
| --- | --- |
| CPU and slow periods | Hot call paths, time distribution, slow periods |
| Entities and chunks | Entity distribution and chunk statistics |
| Memory and GC | Memory usage, GC activity, heap object rankings |
| AI troubleshooting | Suggestions based on report data, with follow-up questions |

Available data depends on the report type and what was captured.

### Getting started

**1. Choose how to run it**

| Option | Use it for |
| --- | --- |
| [Web app](https://bro-know-my-org.github.io/BroKnowMySparkAnalyzer/) | Import and analyze reports in a browser, with no installation |
| [Desktop app](https://github.com/bro-know-my-org/BroKnowMySparkAnalyzer/releases) | Use system credential storage and native save dialogs |
| `bkmsa` CLI | Analyze reports on a server or automate analysis in scripts |

**2. Import a report**

Export a file from spark or copy a report link / key. Supported files include `.sparkprofile`, `.sparkheap`, `.sparkhealth`, and raw health protobuf. Paste text logs into the text input in the web or desktop app. You can explore the report without configuring an AI provider.

**3. Enable AI when needed**

Choose or configure an OpenAI-compatible provider in settings, enter your API key, and check the base URL and model. You can then start an analysis or ask follow-up questions.

Basic report analysis runs locally, including file parsing in the web app. When you use AI, data needed for analysis is sent to the provider you configure.

### CLI

Download a `bkmsa` binary for your platform from [Releases](https://github.com/bro-know-my-org/BroKnowMySparkAnalyzer/releases), or install with Rust:

```bash
cargo install bkmsa-cli

# Read a report summary
bkmsa inspect report.sparkprofile

# Find expensive call paths
bkmsa tool report.sparkprofile hot-paths --category auto --limit 16

# Export JSON for other programs
bkmsa inventory report.sparkprofile --format json
```

The analysis commands above work without an API key. See the [CLI guide](docs/CLI.md) for AI analysis, stdin, configuration files, and all options.

### Usage notes

- Browsers may block remote spark links or AI requests because of CORS. If a link fails, download the report and import the file. If AI requests are blocked, use the desktop app or CLI.
- Desktop packages are currently unsigned. Check each Release for macOS DMG availability.

### Documentation

- [CLI](docs/CLI.md): AI configuration, report tools, JSON output, and stdin
- [Development](docs/DEVELOPMENT.md): prerequisites, running, building, and checks
- [Integration](docs/INTEGRATION.md): Vue UI, Rust crates, and the Tauri plugin
- [Maintenance and releases](docs/MAINTAINING.md) (Chinese): test conventions, SDK publishing, and release recovery
