<p align="center">
    <img src="apps/desktop/public/logos/VRCS_Logo.svg" width="50%" alt="logo" />
</p>

# VRCS

[English](README.md) | **简体中文** | [日本語](README.ja-JP.md)

VRCS 是面向 VRChat 场景的 Windows 实时字幕与语言学习工具。它可以采集系统输出、VRChat 进程音频和麦克风，在桌面或 SteamVR 中显示字幕，并将字幕继续用于翻译、查词、学习分析、Anki 制卡和 VRChat Chatbox 输出。

[下载最新版本](https://github.com/Dreaminko/VRCS/releases/latest) · [问题反馈](https://github.com/Dreaminko/VRCS/issues) · [参与贡献](CONTRIBUTING.md)

[Discord](https://discord.gg/53H872eYq) · [QQ 群](https://qm.qq.com/q/i9kOOxFn44)

## 下载与安装

请从 [GitHub Releases](https://github.com/Dreaminko/VRCS/releases) 下载对应安装包：

| 安装包 | 适用场景 | 额外要求 |
|---|---|---|
| `VRCS-<version>-windows-x64.exe` | 推荐给大多数用户；支持云端识别和本地 CPU Whisper | 无需 CUDA |
| `VRCS-<version>-windows-x64-CUDA.exe` | 使用 NVIDIA GPU 加速本地 Whisper | CUDA 12.x Runtime、cuBLAS、兼容的 NVIDIA GPU 与驱动 |

标准版和 CUDA 版共用同一套配置、数据库和模型目录，可以直接互换安装。

运行环境：

- [Microsoft Visual C++ v14 Redistributable（x64）](https://aka.ms/vs/17/release/vc_redist.x64.exe)
- 首次启动时需要联网下载固定版本的 Silero VAD 模型
- 使用本地 Whisper 时，需要按所选模型完成首次下载
- 使用云端识别、翻译或学习分析时，需要对应服务商的 API 凭据；服务商可能产生费用

## 首次使用

[使用阿里巴巴百炼免费额度快速开始](./docs/AlibabaCloud_Free.md)

首次启动会进入设置向导：

1. 选择简体中文、日语、英语或跟随系统语言。
2. 选择云端实时识别或本地 Whisper。
3. 配置系统音频、VRChat 进程音频和麦克风。
4. 测试麦克风并校准语音触发阈值。
5. 完成设置并开始转写。

配置可以随时在应用内重新调整，首次使用向导也可以从“设置 → 系统”再次运行。

## 主要功能

### 实时字幕与音频

- Windows WASAPI 系统回环、VRChat 进程专用回环和麦克风捕获
- 系统音频与麦克风双路转写，可分别控制音频源和设备
- Silero ONNX VAD；模型不可用时自动回退到能量检测
- 实时增量字幕、最终字幕、会话历史和紧凑窗口模式
- 字幕历史保存在本机 SQLite，可按会话组织、重命名和清理

### 语音识别

- 本地 `whisper.cpp`，支持 CPU 和可选 CUDA 加速
- 本地 Whisper 模型下载、完整性校验、迁移和删除
- Alibaba Cloud Qwen3 ASR 与 Fun-ASR 实时流式识别
- OpenAI Realtime Transcription
- 云端断线重连和可配置的失败处理策略

### 翻译与上下文

- 手动翻译或自动翻译
- DeepL、Microsoft Translator、OpenAI、Gemini、Alibaba Cloud LLM
- OpenAI 兼容 Chat Completions 服务，包括 DeepSeek、Groq、OpenRouter、LM Studio、Ollama 和自定义端点
- 自定义系统 Prompt、本地术语表、在线术语订阅和最近字幕上下文
- 可选读取本机 [VRCX-0](https://vrcx-0.dev/) 的当前世界名、成员显示名和成员语言，为支持的 ASR 或 LLM 请求补充上下文

### VRChat 与 SteamVR

- 将自己的麦克风最终字幕和译文发送到 VRChat OSC Chatbox
- Chatbox 快速输入、翻译预览、格式设置和 144 字符处理
- 通过 OSCQuery 同步 VRChat 的 `MuteSelf` 状态；静音或状态未知时阻止自动发送
- SteamVR VR Overlay：头显字幕与手腕对话视图
- Overlay 支持原文、译文或双语内容，可选择系统音频、麦克风和 Chatbox 来源，并调整位置、尺寸、透明度和显示时间

### 查词、学习与 Anki

- 导入和管理 Yomitan 词典包
- 在字幕中划词查询，并保留原句和译文上下文
- 从实时字幕、历史字幕和查词结果收集学习素材
- 使用所选 LLM 执行上下文词义解释、句型分析和会话回顾
- 编辑词汇卡、句型卡和完形填空卡草稿
- 通过 AnkiConnect 选择牌组、笔记类型和字段映射后制卡

## 隐私与数据

VRCS 不保存原始音频。字幕历史、会话、学习项目、词典和配置默认保存在本机。

使用本地 Whisper 时，语音不会发送到云端。使用云端识别时，检测到的语音片段会发送给所选识别服务商；使用云端翻译或学习分析时，相关文本和用户明确选择的上下文会发送给对应服务商。

## 从源码运行

开发环境：

- Windows 10 / 11
- Node.js 24+
- Rust stable
- Visual Studio Build Tools，并安装“使用 C++ 的桌面开发”工作负载
- 仅 CUDA 开发需要 NVIDIA CUDA Toolkit，并设置 `CUDA_PATH`

```powershell
npm install
npm run dev
```

默认命令使用 CPU 构建。启用 CUDA：

```powershell
npm run dev:cuda
```

只运行独立 Rust Core：

```powershell
npm run dev:core
npm run dev:core:cuda
```

独立 Core 默认监听 `http://127.0.0.1:8766`，字幕 WebSocket 为 `ws://127.0.0.1:8766/ws`。桌面应用会自动生成并管理本地会话令牌；如果单独运行 Core 并监听非回环地址，必须显式设置非空的 `VRCS_SESSION_TOKEN`。

## 测试

```powershell
npm run check:i18n
npm --workspace apps/desktop test
npm run build:frontend
.\scripts\test-core.ps1
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
```

## 构建 Release

构建标准 Windows 安装包：

```powershell
npm run build
```

同时构建标准版和 CUDA 版：

```powershell
.\scripts\build-release.ps1 -Version 0.1.0 -IncludeCuda
```

## 参与贡献

请先阅读 [CONTRIBUTING.md](CONTRIBUTING.md)。新增或更新界面语言请参阅 [LOCALIZATION.md](LOCALIZATION.md)。提交前请运行与改动相关的测试，不要提交生成的构建产物。

## License

VRCS 使用 [GNU Affero General Public License v3.0](LICENSE)（`AGPL-3.0-only`）。第三方组件及其许可信息见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
