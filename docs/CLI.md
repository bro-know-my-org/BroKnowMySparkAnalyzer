# 命令行 / CLI

[项目首页](../README.md) · [中文](#中文) · [English](#english)

## 中文

安装需要 Rust，也可以从 [Releases](https://github.com/bro-know-my-org/BroKnowMySparkAnalyzer/releases) 下载对应平台的 `bkmsa` 二进制。

```bash
cargo install bkmsa-cli

# 查看报告摘要
bkmsa inspect report.sparkprofile

# 查看热点调用路径
bkmsa tool report.sparkprofile hot-paths --category auto --limit 16

# 输出 JSON，方便交给其他程序处理
bkmsa inventory report.sparkprofile --format json
```

AI 分析需要配置服务：

```bash
export BKMSA_API_KEY="your-api-key"
export BKMSA_BASE_URL="https://api.openai.com/v1"
export BKMSA_MODEL="your-model-name"

bkmsa analyze report.sparkprofile --format markdown --output diagnosis.md
```

`inspect` 和 `tool` 不需要 API Key。输入也可以是 spark 链接；用标准输入读取二进制报告时写 `-`，读取文本日志时再加 `--text`。

```bash
cat report.sparkprofile | bkmsa inspect - --format json
cat server.log | bkmsa inspect - --text --format json
```

通过 `bkmsa tools` 查看可用工具，`bkmsa --help` 和 `bkmsa <命令> --help` 查看完整参数。工具名支持 `hot_paths` 和 `hot-paths` 两种写法，复杂参数可用 `--args '{"limit":12}'`，也可逐项传 `--arg KEY=VALUE`。

配置可以放在系统配置目录下的 `bkmsa/config.toml`，或用 `--config` / `BKMSA_CONFIG` 指定路径。命令行和环境变量优先于配置文件。API Key 建议通过 `BKMSA_API_KEY` 提供。

## English

Install with Rust, or download a `bkmsa` binary for your platform from [Releases](https://github.com/bro-know-my-org/BroKnowMySparkAnalyzer/releases).

```bash
cargo install bkmsa-cli

# Read a report summary
bkmsa inspect report.sparkprofile

# Find expensive call paths
bkmsa tool report.sparkprofile hot-paths --category auto --limit 16

# Export JSON for other programs
bkmsa inventory report.sparkprofile --format json
```

Configure a provider for AI analysis:

```bash
export BKMSA_API_KEY="your-api-key"
export BKMSA_BASE_URL="https://api.openai.com/v1"
export BKMSA_MODEL="your-model-name"

bkmsa analyze report.sparkprofile --format markdown --output diagnosis.md
```

`inspect` and `tool` work without an API key. You can also pass a spark link. Use `-` to read a binary report from stdin, and add `--text` for text logs:

```bash
cat report.sparkprofile | bkmsa inspect - --format json
cat server.log | bkmsa inspect - --text --format json
```

Run `bkmsa tools` to list tools, and `bkmsa --help` or `bkmsa <command> --help` for all options. Tool names accept both `hot_paths` and `hot-paths`. Pass arguments with `--args '{"limit":12}'` or individual `--arg KEY=VALUE` options.

Configuration can live in `bkmsa/config.toml` under your system config directory, or a file selected with `--config` / `BKMSA_CONFIG`. Command-line options and environment variables take precedence over the file. Use `BKMSA_API_KEY` to supply your key.
