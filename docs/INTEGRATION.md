# 项目集成 / Integration

[项目首页](../README.md) · [中文](#中文) · [English](#english)

## 中文

解析、报告工具和 AI 分析由 Rust 实现，网页通过 WASM 调用，桌面通过 Tauri 调用，CLI 直接使用同一套核心。Vue 界面也可以单独嵌入其他应用。

| 包 | 文档 |
| --- | --- |
| `@bro-know-my/spark-analyzer` | [Vue 界面、adapter 和嵌入示例](../packages/spark-analyzer/README.md) |
| `bkmsa-core` | [报告解析和分析工具](../crates/bkmsa-core/README.md) |
| `bkmsa-agent` | [AI 分析](../crates/bkmsa-agent/README.md) |
| `bkmsa-tauri` | [Tauri 插件和宿主授权](../crates/bkmsa-tauri/README.md) |
| `bkmsa-cli` | [命令行安装](../crates/bkmsa-cli/README.md) |

## English

Rust handles parsing, report tools, and AI analysis. The web app calls it through WASM, the desktop app through Tauri, and the CLI uses the same core directly. The Vue UI can also be embedded in other applications.

| Package | Documentation |
| --- | --- |
| `@bro-know-my/spark-analyzer` | [Vue UI, adapters, and embedding examples](../packages/spark-analyzer/README.md) |
| `bkmsa-core` | [Report parsing and analysis tools](../crates/bkmsa-core/README.md) |
| `bkmsa-agent` | [AI analysis](../crates/bkmsa-agent/README.md) |
| `bkmsa-tauri` | [Tauri plugin and host authorization](../crates/bkmsa-tauri/README.md) |
| `bkmsa-cli` | [CLI installation](../crates/bkmsa-cli/README.md) |
