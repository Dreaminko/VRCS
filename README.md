# VRCS

VRCS 是一个本地优先的 VRChat 字幕学习工具。它捕获 Windows 系统输出或 VRChat 进程音频，并可同时捕获麦克风；语音始终在本机切分，可选择本地 Whisper 或 Qwen/Fun-ASR/OpenAI 云端流式识别，字幕实时显示在桌面端，也可以查词并发送到本机 Anki。

## 项目状态

当前为 0.1 基础实现。Rust Core 已替换原 Python Core，并直接嵌入 Tauri 主进程；音频采集、VAD、Whisper、字幕存储、实时推送与字幕翻译链路已经接通。VR Overlay 和说话人分离仍在后续路线图中。

## 当前功能

- Windows WASAPI 系统回环、VRChat 进程专用回环与麦克风双路捕获
- 手动或自动字幕翻译，支持 DeepL、Microsoft Translator、OpenAI 与 Alibaba Cloud LLM
- Silero ONNX VAD，首次启动自动下载并校验固定版本，模型不可用时使用能量检测回退
- whisper.cpp 本地 CPU/CUDA 转写、自动 GPU 回退与 GGML 模型管理
- Qwen3 ASR、Fun-ASR 与 OpenAI 实时流式转写，支持增量字幕、断线重连和本地回退
- Axum HTTP 接口和 WebSocket 字幕推送
- SQLite 字幕历史、Yomitan 词典导入和内置英日测试词典
- React + Tauri 桌面端，包括实时字幕、历史、识别设置和音频设备页面
- 可跟随系统或手动切换的简体中文、日语、英语界面
- AnkiConnect 连接、牌组/笔记类型/字段映射与一键制卡

完整进度见[路线图](docs/roadmap.md)。

## 技术栈

Core 使用 Rust、Axum、Tokio、SQLite、WASAPI、ONNX Runtime 和 whisper.cpp。桌面端使用 TypeScript、React、Vite 与 Tauri 2。Core 作为 Rust 库嵌入桌面主进程，同时保留独立二进制用于 API 调试。

## 运行 Release 安装版

Release 安装包面向 Windows 10/11 x64。默认下载不带 `-CUDA` 后缀的统一客户端；它同时支持云端识别和本地 CPU Whisper，不需要安装 CUDA。运行安装版需要：

- **Microsoft Visual C++ v14 Redistributable（x64）**：安装[微软当前支持的最新 x64 版本](https://aka.ms/vc14/vc_redist.x64.exe)。
- **Microsoft Edge WebView2 Evergreen Runtime**：Windows 11 和已更新的 Windows 10 通常已包含；安装器会在缺失时联网安装。
- **网络连接（首次启动与首次识别模型）**：首次启动下载 Silero VAD v6.2.1，首次使用某个 Whisper 模型时下载该模型；文件保存在 `%LOCALAPPDATA%\.vrcs\models`。

文件名带 `-CUDA` 后缀的可选安装包额外提供 NVIDIA CUDA 加速。该版本按 CUDA 12.4.1 构建，需要用户自行安装 CUDA 12.x Runtime、cuBLAS、cuBLASLt，相关 DLL 必须位于系统 `PATH`，并使用兼容的 NVIDIA GPU 和 551.78 或更高版本驱动。标准版和 CUDA 版使用相同配置、数据库和模型目录，可以直接互换安装。

两个安装版都不附带 NVIDIA CUDA 运行库，也不需要另行安装 Python、Node.js、Rust 或 FFmpeg。Anki 和 AnkiConnect 只在使用制卡功能时需要。

## 从源码开发

开发环境需要 Windows 10/11、Node.js 20+、Rust stable，以及安装了“使用 C++ 的桌面开发”工作负载的 Visual Studio Build Tools。只有启用 CUDA 开发命令时才需要 NVIDIA CUDA Toolkit，并设置 `CUDA_PATH`。

```powershell
npm install
npm run dev
```

Tauri 会在同一进程内启动 Rust Core，退出桌面端时一并停止。开发模式 Core 默认监听 `http://127.0.0.1:8766`，WebSocket 为 `ws://127.0.0.1:8766/ws`；配置、数据库和模型保存在 `%LOCALAPPDATA%\.vrcs`。

`npm run dev` 和 `npm run dev:core` 默认不启用 CUDA。需要 CUDA 加速时显式使用：

```powershell
npm run dev:cuda
npm run dev:core:cuda
```

只调试后端 API：

```powershell
npm run dev:core
```

独立 Core 默认使用 `core/config.json`，也支持 `VRCS_CONFIG`、`VRCS_HOST`、`VRCS_PORT`、`VRCS_SESSION_TOKEN`、`VRCS_SILERO_MODEL`、`VRCS_ASR_MODEL_DIR`、`VRCS_QWEN_API_KEY`、`VRCS_OPENAI_API_KEY`、`VRCS_DEEPL_API_KEY` 和 `VRCS_MICROSOFT_TRANSLATOR_KEY`。同时兼容 `DASHSCOPE_API_KEY`、`OPENAI_API_KEY` 与 `DEEPL_API_KEY`；VRCS 专用变量优先。设置页可以为同一供应商保存多个命名 API 配置，API Key 分别写入 Windows 凭据管理器。环境变量会覆盖该供应商当前配置的已保存密钥，但区域和 Workspace 仍取自当前配置。未设置 `VRCS_SESSION_TOKEN` 时会为回环监听生成临时 token 并输出到终端；监听非回环地址时必须显式设置非空 token。

如果绕过 Tauri、单独运行 Vite 前端，请把同一个 token 同时设置为 `VRCS_SESSION_TOKEN` 和 `VITE_VRCS_SESSION_TOKEN`。

“仅采集 VRChat 音频”使用 Windows 进程回环 API，需要 Windows 10 Build 20348 或更高版本；开启前请先运行 VRChat。

## 测试

```powershell
.\scripts\test-core.ps1
npm --workspace apps/desktop test
npm run check:i18n
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
```

## 构建 Windows Release

```powershell
npm run build
```

构建脚本校验 `tauri.conf.json`、桌面端 `Cargo.toml` 和 Core `Cargo.toml` 的版本，执行测试，默认生成不依赖 CUDA 的统一客户端，并对生成的桌面程序执行一次 Core 与 Silero 首启下载自检，最后生成 NSIS 安装包与 SHA-256。也可显式指定版本：

```powershell
.\scripts\build-release.ps1 -Version 0.1.0
```

需要同时构建可选 CUDA 版本时使用：

```powershell
.\scripts\build-release.ps1 -Version 0.1.0 -IncludeCuda
```

规范化产物位于 `release-artifacts/`：标准版为 `VRCS-<version>-windows-x64.exe`，CUDA 版为 `VRCS-<version>-windows-x64-CUDA.exe`。推送匹配的标签（例如 `v0.1.0`）会触发 Windows GitHub Actions，在同一个 Draft Release 中发布两个安装包及其 SHA-256。

## OpenAI 兼容 LLM

字幕翻译支持 OpenAI 兼容的 Chat Completions API。前往“设置 → API 管理”，新增 `OpenAI / Compatible` 配置并填写 Base URL；例如 DeepSeek 可使用 `https://api.deepseek.com/v1`。保存 API Key 后，再到“字幕翻译”中选择该配置并填写服务商支持的模型名（例如 `deepseek-chat`）。

Base URL 留空时使用 OpenAI 官方 Responses API，并保留 OpenAI Realtime 语音识别能力；填写自定义 Base URL 后，该配置仅用于 LLM 翻译，VRCS 会请求 `{Base URL}/chat/completions`。Base URL 也可以直接填写完整的 `/chat/completions` 地址。

配置了 API Key 后，VRCS 会自动请求对应的 `/models` 接口获取可用模型；可在“API 管理”中手动刷新，字幕翻译的模型输入框也会提供获取到的模型建议。如果服务商未实现模型列表接口，仍可直接手动填写模型名。

## AnkiConnect

在 Anki 中安装 AnkiConnect 并保持 Anki 运行。VRCS 默认连接 `http://127.0.0.1:8765`；如果修改过端口，可在“设置 → Anki”中同步修改。

## 隐私

VRCS 默认使用 Qwen3 ASR 云端流式识别，也可切换到 Fun-ASR、OpenAI 或完全本地的 Whisper。VRCS 不保存原始音频；使用云端识别时，检测到语音的 PCM 片段会发送给所选服务商，字幕历史仍保存在本机。录制或上传他人语音前，请确认符合 VRChat 社区规则、服务商条款和当地法律。详见[隐私说明](docs/privacy.md)。

## 文档

- [架构](docs/architecture.md)
- [开发说明](docs/development.md)
- [本地化贡献指南](LOCALIZATION.md)
- [隐私说明](docs/privacy.md)
- [路线图](docs/roadmap.md)

## License

[MIT](LICENSE)
