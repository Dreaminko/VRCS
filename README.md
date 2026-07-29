# VRCS

VRCS 是一个本地优先的 VRChat 字幕学习工具。它捕获 Windows 系统输出或 VRChat 进程音频，并可同时捕获麦克风；语音在本机切分和转写，字幕实时显示在桌面端，也可以查词并发送到本机 Anki。

## 项目状态

当前为 0.1 基础实现。Rust Core 已替换原 Python Core，并直接嵌入 Tauri 主进程；音频采集、VAD、Whisper、字幕存储与实时推送链路已经接通。VR Overlay、说话人分离和翻译仍在后续路线图中。

## 当前功能

- Windows WASAPI 系统回环、VRChat 进程专用回环与麦克风双路捕获
- Silero ONNX VAD，模型不可用时自动使用能量检测回退
- whisper.cpp 本地 CPU/CUDA 转写、自动 GPU 回退与 GGML 模型管理
- Axum HTTP 接口和 WebSocket 字幕推送
- SQLite 字幕历史、Yomitan 词典导入和内置英日测试词典
- React + Tauri 桌面端，包括实时字幕、历史、识别设置和音频设备页面
- AnkiConnect 连接、牌组/笔记类型/字段映射与一键制卡

完整进度见[路线图](docs/roadmap.md)。

## 技术栈

Core 使用 Rust、Axum、Tokio、SQLite、WASAPI、ONNX Runtime 和 whisper.cpp。桌面端使用 TypeScript、React、Vite 与 Tauri 2。Core 作为 Rust 库嵌入桌面主进程，同时保留独立二进制用于 API 调试。

## 运行 Release 安装版

Release 安装包面向 Windows 10/11 x64。运行安装版需要：

- **Microsoft Visual C++ v14 Redistributable（x64）**：安装[微软当前支持的最新 x64 版本](https://aka.ms/vc14/vc_redist.x64.exe)。
- **Microsoft Edge WebView2 Evergreen Runtime**：Windows 11 和已更新的 Windows 10 通常已包含；安装器会在缺失时联网安装。
- **NVIDIA CUDA Runtime（安装版必需）**：官方安装包按 CUDA 12.4.1 构建，需要用户自行安装 CUDA 12.x Runtime、cuBLAS、cuBLASLt，相关 DLL 必须位于系统 `PATH`。CUDA 推理还需要兼容的 NVIDIA GPU 和 551.78 或更高版本驱动；选择 CPU 只停用 GPU 推理，不会移除程序启动时的 CUDA DLL 依赖。
- **网络连接（首次识别模型）**：标准安装包不包含 Whisper 模型，下载后保存在 `%LOCALAPPDATA%\.vrcs\models\whisper`。

安装版不附带 NVIDIA CUDA 运行库，也不需要另行安装 Python、Node.js、Rust 或 FFmpeg。Anki 和 AnkiConnect 只在使用制卡功能时需要。

## 从源码开发

开发环境需要 Windows 10/11、Node.js 20+、Rust stable、NVIDIA CUDA Toolkit（设置 `CUDA_PATH`），以及安装了“使用 C++ 的桌面开发”工作负载的 Visual Studio Build Tools。

```powershell
npm install
npm run dev
```

Tauri 会在同一进程内启动 Rust Core，退出桌面端时一并停止。开发模式 Core 默认监听 `http://127.0.0.1:8766`，WebSocket 为 `ws://127.0.0.1:8766/ws`；配置、数据库和模型保存在 `%LOCALAPPDATA%\.vrcs`。

`npm run dev` 和 `npm run dev:core` 默认启用 CUDA；无 Toolkit 的 CPU-only 开发可分别使用 `npm run dev:desktop:cpu` 和 `npm run dev:core:cpu`。

只调试后端 API：

```powershell
npm run dev:core
```

独立 Core 默认使用 `core-rust/config.json`，也支持 `VRCS_CONFIG`、`VRCS_HOST`、`VRCS_PORT`、`VRCS_SESSION_TOKEN`、`VRCS_SILERO_MODEL` 和 `VRCS_ASR_MODEL_DIR`。监听非回环地址时必须设置非空的 `VRCS_SESSION_TOKEN`。

“仅采集 VRChat 音频”使用 Windows 进程回环 API，需要 Windows 10 Build 20348 或更高版本；开启前请先运行 VRChat。

## 测试

```powershell
.\scripts\test-core.ps1
npm --workspace apps/desktop test
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
```

## 构建 Windows Release

```powershell
npm run build
```

构建脚本校验 `tauri.conf.json`、桌面端 `Cargo.toml` 和 Core `Cargo.toml` 的版本，执行测试，以 CUDA 特性构建后生成不含 NVIDIA 运行库的 NSIS 安装包与 SHA-256。也可显式指定版本：

```powershell
.\scripts\build-release.ps1 -Version 0.1.0
```

产物位于 `apps/desktop/src-tauri/target/release/bundle/nsis/`。推送匹配的标签（例如 `v0.1.0`）会触发 Windows GitHub Actions 并创建 Draft Release。

## AnkiConnect

在 Anki 中安装 AnkiConnect 并保持 Anki 运行。VRCS 默认连接 `http://127.0.0.1:8765`；如果修改过端口，可在“设置 → Anki”中同步修改。

## 隐私

音频、转写和存储均在本机完成。VRCS 默认不保存原始音频，也不会上传音频。录制他人语音前，请确认符合 VRChat 社区规则和当地法律。详见[隐私说明](docs/privacy.md)。

## 文档

- [架构](docs/architecture.md)
- [开发说明](docs/development.md)
- [隐私说明](docs/privacy.md)
- [路线图](docs/roadmap.md)

## License

[MIT](LICENSE)
