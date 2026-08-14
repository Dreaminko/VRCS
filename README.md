# VRCS

VRCS 是一个本地优先的 VRChat 字幕学习工具。它捕获 Windows 系统输出或 VRChat 进程音频，并可同时捕获麦克风；语音始终在本机切分，可选择本地 Whisper 或 Qwen/Fun-ASR/OpenAI 云端流式识别，字幕实时显示在桌面端，也可以查词并发送到本机 Anki。

## 项目状态

当前为 0.1 基础实现。Rust Core 已替换原 Python Core，并直接嵌入 Tauri 主进程；音频采集、VAD、Whisper、字幕存储、实时推送与字幕翻译链路已经接通。VR Overlay 和说话人分离仍在后续路线图中。

## 当前功能

- Windows WASAPI 系统回环、VRChat 进程专用回环与麦克风双路捕获
- 手动或自动字幕翻译，支持 DeepL、Microsoft Translator、OpenAI、Gemini 与 Alibaba Cloud LLM
- 将自己的麦克风最终识别结果和译文通过 OSC 发送到 VRChat Chatbox，并同步 VRChat 麦克风静音状态作为发送安全门
- Silero ONNX VAD，首次启动自动下载并校验固定版本，模型不可用时使用能量检测回退
- whisper.cpp 本地 CPU/CUDA 转写、自动 GPU 回退与 GGML 模型管理
- Qwen3 ASR、Fun-ASR 与 OpenAI 实时流式转写，支持增量字幕、断线重连和本地回退
- Axum HTTP 接口和 WebSocket 字幕推送
- 默认关闭、独立监听且版本化的第三方事件 WebSocket API
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

开发环境需要 Windows 10/11、Node.js 24+、Rust stable，以及安装了“使用 C++ 的桌面开发”工作负载的 Visual Studio Build Tools。只有启用 CUDA 开发命令时才需要 NVIDIA CUDA Toolkit，并设置 `CUDA_PATH`。

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

独立 Core 默认使用 `core/config.json`，也支持 `VRCS_CONFIG`、`VRCS_HOST`、`VRCS_PORT`、`VRCS_SESSION_TOKEN`、`VRCS_EXTERNAL_API_TOKEN`、`VRCS_SILERO_MODEL`、`VRCS_ASR_MODEL_DIR`、`VRCS_QWEN_API_KEY`、`VRCS_OPENAI_API_KEY`、`VRCS_GEMINI_API_KEY`、`VRCS_OPENAI_COMPATIBLE_API_KEY`、`VRCS_DEEPL_API_KEY` 和 `VRCS_MICROSOFT_TRANSLATOR_KEY`。同时兼容 `DASHSCOPE_API_KEY`、`OPENAI_API_KEY`、`GEMINI_API_KEY` 与 `DEEPL_API_KEY`；VRCS 专用变量优先。设置页可以为同一供应商保存多个命名 API 配置，API Key 分别写入 Windows 凭据管理器。环境变量会覆盖该供应商当前配置的已保存密钥，但区域和 Workspace 仍取自当前配置。未设置 `VRCS_SESSION_TOKEN` 时会为回环监听生成临时 token 并输出到终端；监听非回环地址时必须显式设置非空 token。

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

字幕翻译支持 OpenAI 兼容的 Chat Completions API。前往“设置 → API 管理”，新增“OpenAI 兼容（仅 LLM）”配置，可选择 DeepSeek、Groq、OpenRouter、LM Studio 或 Ollama 预设，也可以使用 Custom 手动配置。预设会填入稳定保存的 Base URL、鉴权方式和本地服务标记。

OpenAI 官方连接可按需设为“仅语音识别”“仅 LLM / 翻译”或共享，并通过 OpenAI Responses API 提供 LLM 翻译。OpenAI 兼容连接只支持 LLM；请求发送到 `{Base URL}/chat/completions`，也可以直接填写完整的 `/chat/completions` 地址。LM Studio 和 Ollama 预设默认不要求 API Key；云端服务默认使用 Bearer API Key，使用 Bearer 时远程地址必须采用 HTTPS。

满足所选鉴权方式后，VRCS 会自动请求对应的 `/models` 接口获取可用模型；可在“API 管理”中手动刷新，字幕翻译的模型输入框也会提供获取到的模型建议。如果服务商未实现模型列表接口，仍可直接手动填写模型名。连接诊断会分别检查网络、鉴权、模型列表、普通生成和 SSE 流式生成。Custom Profile 可设置 1–120 秒超时及非敏感自定义 Header；Header 会以明文写入本地配置，不能用于保存密钥或令牌。

## Gemini 原生 API

前往“设置 → API 管理”新增 Gemini 配置并保存 Google AI Studio API Key。VRCS 使用 Gemini 原生 `generateContent`、`streamGenerateContent` 和模型列表接口，不经 OpenAI 兼容层；模型列表只显示支持文本生成的模型。也可以通过 `VRCS_GEMINI_API_KEY` 或 `GEMINI_API_KEY` 提供密钥。

## LLM 翻译增强

使用 OpenAI、Gemini、Alibaba Cloud 或 OpenAI Compatible 翻译时，“设置 → 翻译”可编辑系统 Prompt、维护术语表，并选择是否附带最近的扬声器、麦克风和 Chatbox 原文。上下文默认关闭，可分别控制三个来源，并受消息数和字符数双重限制；本地预览会显示实际附带的条数、字符数和最终系统指令。DeepL 与 Microsoft Translator 不会接收这些增强内容。

## 第三方输出 API

“设置 → 系统”可启用独立的只读事件 WebSocket。该 API 默认关闭并监听 `127.0.0.1:8767`，通过订阅输出 ASR partial/final、翻译生命周期和 Chatbox 发送结果；不会暴露内部设置或控制端点。非回环监听必须开启独立 Token 鉴权，监听配置修改后需重启应用。协议与客户端示例见[第三方输出 API v1](docs/external-api-v1.md)。

## AnkiConnect

在 Anki 中安装 AnkiConnect 并保持 Anki 运行。VRCS 默认连接 `http://127.0.0.1:8765`；如果修改过端口，可在“设置 → Anki”中同步修改。

## OSC Chatbox

在“设置 → OSC”中启用聊天框输出，默认向本机 UDP 9000 端口发送消息。同时需要在 VRChat 的动作菜单中打开 `OSC → Enabled`。自动输出只发送自己的麦克风最终字幕，不发送其他人的语音或流式临时结果。

主页面底栏中，“开始转写”左侧的 Chatbox 按钮会在底栏上方打开快速输入栏。默认点击发送后调用当前翻译 Provider，并将原文和译文一起发送；输入栏左侧可向上展开目标语言、发送内容、消息格式、译文编辑和 144 字符处理设置，发送设置会自动保存在本机。发送成功以 UDP 实际写入为准，翻译或发送失败时保留当前草稿并显示原因。

“同步 VRChat 麦克风静音”默认开启。VRCS 启动后会通过本机 OSCQuery 主动读取 `MuteSelf`，VRChat 静音时立即暂停麦克风转写并阻止 Chatbox 输出，取消静音后自动恢复。静音状态未知时发送门保持关闭，状态栏会显示当前原因；如不需要该联动，可在同一设置页关闭。可选的应用内静音状态提示默认关闭。

如需向聊天框同时发送译文，请在“设置 → 翻译 → 自己的声音”中启用“自动翻译自己说的话”，并选择对方使用的目标语言。该设置独立于其他声音的自动翻译方向，例如可以把其他人的日语翻译成中文，同时把自己的中文翻译成日语。

## 隐私

VRCS 默认使用 Qwen3 ASR 云端流式识别，也可切换到 Fun-ASR、OpenAI 或完全本地的 Whisper。VRCS 不保存原始音频；使用云端识别时，检测到语音的 PCM 片段会发送给所选服务商，字幕历史仍保存在本机。录制或上传他人语音前，请确认符合 VRChat 社区规则、服务商条款和当地法律。详见[隐私说明](docs/privacy.md)。

## 文档

- [架构](docs/architecture.md)
- [开发说明](docs/development.md)
- [本地化贡献指南](LOCALIZATION.md)
- [隐私说明](docs/privacy.md)
- [第三方输出 API v1](docs/external-api-v1.md)
- [路线图](docs/roadmap.md)

## License

[GNU Affero General Public License v3.0](LICENSE) (`AGPL-3.0-only`)
