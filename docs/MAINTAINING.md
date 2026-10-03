# 维护与发布

日常使用见 [项目首页](../README.md)，环境准备、构建和检查见 [本地开发](DEVELOPMENT.md)。

## 测试策略

仓库不能依赖真实服务器报告或私有 `.sparkprofile` fixture。核心测试因此使用合成 protobuf/`Report` 数据验证解析、分类、证据与诊断契约；CLI 集成测试验证命令、JSON envelope 和退出码；agent 测试使用 mock provider，不调用真实 AI 服务。

私有真实报告只适合本地补充回归，不能提交到仓库。发现真实报告暴露的新结构时，应先最小化并匿名化为合成 fixture 或结构化契约测试，再修复 Rust 核心。旧的 `scripts/*.mjs` 只保留为 `bkmsa` CLI 兼容包装，不含第二套解析/分析逻辑。

## 发布资产

推送 tag 前必须先把 workspace 版本更新为目标版本。`Publish SDK Packages` workflow 会按依赖顺序发布 `bkmsa-core`、`bkmsa-agent`、`bkmsa-tauri`、`bkmsa-cli`，然后发布同版本的 `@bro-know-my/spark-analyzer`。Actions secrets 只需要：

```text
CARGO_REGISTRY_TOKEN  crates.io 发布 token
```

npm 使用 [Trusted Publishing](https://docs.npmjs.com/trusted-publishers) 的 OIDC 身份发布，不需要 `NPM_TOKEN`。在 npm 包的 Settings → Trusted Publisher 中选择 GitHub Actions，填写：

| 字段 | 值 |
| --- | --- |
| Organization or user | `bro-know-my-org` |
| Repository | `BroKnowMySparkAnalyzer` |
| Workflow filename | `release.yml` |
| Environment name | 留空 |
| Allowed actions | 允许 `npm publish` |

信任的是调用方 `release.yml`，不是可复用工作流 `publish-npm.yml`。两层工作流都已授予 `id-token: write`；发布任务使用 npm 11.19.0。新建信任配置时需明确允许直接发布，否则只允许暂存发布。

首次 `cargo publish` 会自动创建 crate，不需要在 crates.io 手动建包。首次发布后可以把 token 收紧到这四个 crate，或迁移到 crates.io Trusted Publishing。crate 版本不可覆盖；workflow 支持安全重跑并跳过已经存在的同版本包。

推送到 `master` 会在 WASM smoke test 和 Web 构建成功后更新 GitHub Pages。推送严格的 `vX.Y.Z` tag 会先校验已提交版本并运行完整 CI，再构建 Windows、Linux 和 macOS 桌面包及原生 CLI；随后发布 Rust/npm SDK、部署网页并创建 GitHub Release。

如果发布阶段失败，可在 `master` 上手动触发 Release workflow，填写现有 `release_tag` 和原 tag 发版任务的 `source_run_id`。恢复入口核对 tag 提交、原任务的版本/CI/四平台构建结果、未过期的四组产物和 Release 草稿，再发布 SDK 并复用原产物完成 GitHub Release；不会移动 tag 或重建桌面包。预期资产如下，`<version>` 为发布版本：

```text
bkmsa-<version>-windows-x64.exe
bkmsa-<version>-linux-x64
bkmsa-<version>-macos-x64
bkmsa-<version>-macos-arm64

bro-know-my-spark-analyzer-<version>-windows-x64-portable.exe
bro-know-my-spark-analyzer-<version>-windows-x64-setup.exe
bro-know-my-spark-analyzer-<version>-windows-x64.msi
bro-know-my-spark-analyzer-<version>-linux-x64.appimage
bro-know-my-spark-analyzer-<version>-linux-x64.deb
bro-know-my-spark-analyzer-<version>-linux-x64.rpm
bro-know-my-spark-analyzer-<version>-macos-x64.dmg
bro-know-my-spark-analyzer-<version>-macos-arm64.dmg
```

macOS DMG 当前是可选产物；若 Tauri 未生成 DMG，release 仍可只包含该平台 CLI。当前公开桌面构建未签名。
