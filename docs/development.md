# 开发说明

## 环境要求

- Windows 10/11 x64
- Node.js 24+
- Rust stable
- Visual Studio Build Tools（“使用 C++ 的桌面开发”工作负载）

“仅采集 VRChat 音频”使用 Windows 进程回环 API，需要 Windows 10 Build 20348 或更高版本。

## 首次安装

```powershell
npm install
cargo check --manifest-path core/Cargo.toml
```

首次启动需要联网下载 Silero VAD 模型；首次使用本地 Whisper 时还会下载所选 GGML 模型。

## 启动开发环境

```powershell
npm run dev
```

Tauri 会在主进程内启动和停止 Rust Core。开发模式 Core 监听 `http://127.0.0.1:8766`，WebSocket 为 `ws://127.0.0.1:8766/ws`；数据保存在 `%LOCALAPPDATA%\.vrcs`。

只调试后端 API：

```powershell
npm run dev:core
```

独立 Core 默认使用当前工作目录的 `config.json`，也可通过 `VRCS_CONFIG`、`VRCS_HOST`、`VRCS_PORT` 和 `VRCS_SESSION_TOKEN` 调整。

## 测试

```powershell
# Rust Core
powershell -ExecutionPolicy Bypass -File scripts/test-core.ps1

# 前端
npm --workspace apps/desktop test

# Tauri Rust 层（同时验证嵌入式 Core 依赖）
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
```

## 构建 Release

```powershell
npm run build
```

构建脚本会校验 Tauri、桌面端 Cargo 和 Core Cargo 的版本，执行全部测试，再生成 NSIS 安装包与 SHA-256。规范化产物位于 `release-artifacts/`。

## 目录结构

```text
apps/desktop/          React 前端与 Tauri 主进程
core/                  可嵌入、可独立运行的 Rust Core
scripts/               发布与测试脚本
docs/                  架构、开发、隐私与路线图
```
