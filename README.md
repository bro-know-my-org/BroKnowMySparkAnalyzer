# BroKnowMySparkAnalyzer

[中文](#中文) · [English](#english)

## 中文

BroKnowMySparkAnalyzer 用来分析 Minecraft [spark](https://spark.lucko.me/) 报告。拿到报告后，可以用它查看耗时最多的调用、卡顿时段、实体和区块统计，以及内存与 GC 情况。也可以接入 AI，让它结合报告中的数据给出排查建议，再针对结果继续追问。

**[打开网页版](https://bro-know-my-org.github.io/BroKnowMySparkAnalyzer/)** · **[下载桌面版](https://github.com/bro-know-my-org/BroKnowMySparkAnalyzer/releases)**

### 开始使用

从 spark 导出报告文件，或复制报告链接，然后在分析器中导入。支持 `.sparkprofile`、`.sparkheap`、`.sparkhealth`、原始 health protobuf 和文本日志，也可以输入 spark 链接或报告 key。

导入后可以查看运行指标、CPU 热点、实体分布和堆内存对象排行。要用 AI 分析，在设置中填入服务地址、API Key 和模型名称即可；支持 OpenAI 兼容接口。

| 方式 | 适合什么情况 |
| --- | --- |
| [网页版](https://bro-know-my-org.github.io/BroKnowMySparkAnalyzer/) | 直接打开浏览器，分析手头的报告 |
| [桌面版](https://github.com/bro-know-my-org/BroKnowMySparkAnalyzer/releases) | 使用本地文件、系统凭据存储和原生保存对话框 |
| `bkmsa` 命令行 | 在服务器上使用，或接入脚本、批处理 |

基础报告分析在本地完成，网页版也在浏览器内解析文件。启用 AI 后，分析所需的数据会发送给你配置的 AI 服务。

网页版读取远程 spark 链接、调用 AI 服务时受浏览器 CORS 限制。链接导入失败时，可以先下载报告再导入；AI 服务不允许浏览器访问时，可以改用桌面版或 CLI。桌面安装包目前未签名，macOS DMG 是否提供以对应 Release 为准。

### 命令行

安装需要 Rust，也可以从 [Releases](https://github.com/bro-know-my-org/BroKnowMySparkAnalyzer/releases) 下载对应平台的 `bkmsa` 二进制。

```bash
cargo install bkmsa-cli
bkmsa inspect report.sparkprofile
bkmsa tool report.sparkprofile hot-paths --category auto --limit 16
```

这些命令不需要 API Key。AI 分析、输出格式、标准输入和配置文件的用法见 [CLI 文档](docs/CLI.md)。

### 更多文档

- [命令行](docs/CLI.md)：AI 配置、报告工具、JSON 导出和标准输入
- [本地开发](docs/DEVELOPMENT.md)：环境准备、运行、构建和检查
- [项目集成](docs/INTEGRATION.md)：Vue 界面、Rust crates 和 Tauri 插件
- [维护与发布](docs/MAINTAINING.md)：测试约定、SDK 发布和发版恢复

## English

BroKnowMySparkAnalyzer helps you read Minecraft [spark](https://spark.lucko.me/) reports: expensive calls, slow periods, entity and chunk statistics, memory usage, and GC. You can also connect an AI provider for troubleshooting suggestions based on the report and ask follow-up questions.

**[Open the web app](https://bro-know-my-org.github.io/BroKnowMySparkAnalyzer/)** · **[Download the desktop app](https://github.com/bro-know-my-org/BroKnowMySparkAnalyzer/releases)**

### Getting started

Export a report from spark or copy its link, then import it into the analyzer. Supported inputs include `.sparkprofile`, `.sparkheap`, `.sparkhealth`, raw health protobuf, text logs, spark links, and report keys.

After importing, review server metrics, CPU hotspots, entity distribution, and heap object rankings. For AI analysis, enter your provider's base URL, API key, and model in settings. Providers must support an OpenAI-compatible API.

| Option | When to use it |
| --- | --- |
| [Web app](https://bro-know-my-org.github.io/BroKnowMySparkAnalyzer/) | Open a browser and analyze a report |
| [Desktop app](https://github.com/bro-know-my-org/BroKnowMySparkAnalyzer/releases) | Use local files, system credential storage, and native save dialogs |
| `bkmsa` CLI | Work on a server or automate analysis in scripts |

Basic report analysis runs locally, including file parsing in the web app. When you enable AI, data needed for analysis is sent to the provider you configure.

Browsers may block remote spark links or AI requests because of CORS. If a link fails, download the report and import the file. If your AI provider blocks browser access, use the desktop app or CLI. Desktop packages are currently unsigned; check each Release for macOS DMG availability.

### CLI

Install with Rust, or download a `bkmsa` binary for your platform from [Releases](https://github.com/bro-know-my-org/BroKnowMySparkAnalyzer/releases).

```bash
cargo install bkmsa-cli
bkmsa inspect report.sparkprofile
bkmsa tool report.sparkprofile hot-paths --category auto --limit 16
```

These commands work without an API key. See the [CLI guide](docs/CLI.md) for AI analysis, output formats, stdin, and configuration files.

### Documentation

- [CLI](docs/CLI.md): AI configuration, report tools, JSON output, and stdin
- [Development](docs/DEVELOPMENT.md): prerequisites, running, building, and checks
- [Integration](docs/INTEGRATION.md): Vue UI, Rust crates, and the Tauri plugin
- [Maintenance and releases](docs/MAINTAINING.md) (Chinese): test conventions, SDK publishing, and release recovery
