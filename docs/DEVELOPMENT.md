# 本地开发 / Development

[项目首页](../README.md) · [中文](#中文) · [English](#english)

## 中文

需要稳定版 Rust、Node.js 22 和 pnpm 11。网页构建还需要 `wasm-pack`；桌面开发需要安装 [Tauri 2 系统依赖](https://v2.tauri.app/start/prerequisites/)。

```bash
pnpm install
```

运行网页版：

```bash
pnpm run build:wasm
pnpm run dev:web
```

运行桌面版：

```bash
pnpm run dev
```

构建与检查：

```bash
pnpm run build
pnpm run check:rust
pnpm run test:rust

# 网页产物生成到 dist/
pnpm run build:site

# 桌面安装包
pnpm run tauri build
```

如果只开发 CLI，可以直接运行 `cargo run -p bkmsa-cli -- inspect report.sparkprofile`。

## English

You need stable Rust, Node.js 22, and pnpm 11. Web builds also need `wasm-pack`; desktop development needs the [Tauri 2 system dependencies](https://v2.tauri.app/start/prerequisites/).

```bash
pnpm install
```

Run the web app:

```bash
pnpm run build:wasm
pnpm run dev:web
```

Run the desktop app:

```bash
pnpm run dev
```

Build and check:

```bash
pnpm run build
pnpm run check:rust
pnpm run test:rust

# Build the web app into dist/
pnpm run build:site

# Package the desktop app
pnpm run tauri build
```

For CLI development, run `cargo run -p bkmsa-cli -- inspect report.sparkprofile` directly.
