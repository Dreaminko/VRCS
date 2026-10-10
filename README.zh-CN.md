<p align="center">
    <img src="apps/desktop/public/logos/VRCS_Logo.svg" width="50%" alt="logo" />
</p>

# VRCS

[English](README.md) | **简体中文** | [繁體中文](README.zh-Hant.md) | [日本語](README.ja-JP.md)

![](./screenshots/01.png)

VRCS 是面向 VRChat 的 Windows 实时字幕、翻译与语言学习工具。采集系统音频和麦克风，在桌面或 SteamVR 中显示字幕；通过 OCR 翻译画面中的文字，并将字幕用于查词、学习分析和 Anki 制卡。

[下载](https://github.com/Dreaminko/VRCS/releases/latest) · [问题反馈](https://github.com/Dreaminko/VRCS/issues) · [参与贡献](CONTRIBUTING.md) · [Discord](https://discord.gg/53H872eYq) · [QQ 群](https://qm.qq.com/q/i9kOOxFn44)

## 安装

从 [GitHub Releases](https://github.com/Dreaminko/VRCS/releases) 下载 `VRCS-<version>-windows-x64.exe`。

- Windows 10 / 11，需安装 [Microsoft Visual C++ v14 Redistributable（x64）](https://aka.ms/vs/17/release/vc_redist.x64.exe)。
- 本地 Qwen ASR 支持 CPU 或 Vulkan，请在识别设置中下载运行组件和模型。
- 首次使用需联网下载 Silero VAD 模型；启用语义断句时还需下载 Smart Turn 模型。
- 云端识别、翻译和学习分析需要对应服务商的 API 凭据，服务商可能收取费用。
- OCR 可选择下载本地模型，或配置 PaddleOCR 云端访问令牌。

## 首次使用

1. 按设置向导选择语言，以及本地或云端识别。
2. 选择系统音频、VRChat 进程音频或麦克风，测试麦克风并调整语音触发阈值。
3. 开始转写，按需启用翻译、Chatbox 输出或 SteamVR 字幕。

可从“设置 › 系统”重新运行向导。云端识别可参考[阿里巴巴百炼免费额度入门](./docs/AlibabaCloud_Free.md)。

## 主要功能

- 系统或 VRChat 进程音频与麦克风双路实时字幕，支持独立音源控制、紧凑窗口和本地会话历史。
- 本地 Qwen3 ASR 支持 CPU 或 Vulkan；云端识别支持 Qwen3 ASR、Fun-ASR、OpenAI Realtime、Gemini 和 Groq。Silero VAD 与可选 Smart Turn 用于语音分段。
- 手动或自动翻译，支持 DeepL、Microsoft Translator、OpenAI、Gemini、Alibaba Cloud LLM 及 OpenAI 兼容服务。系统音频和麦克风可分别配置最多三种目标语言，并为各语言选择服务与模型。
- Qwen Live Translate、Gemini Live Translate（预览）和 OpenAI Realtime Translation 可直接生成实时原文与译文，翻译每路音源的第一目标语言；Qwen 还支持讲话人区分。
- 自定义提示词、本地术语表、在线术语订阅和最近字幕上下文；可选读取 [VRCX-0](https://vrcx-0.dev/) 的世界与成员信息。语言预设可保存并切换识别语言、翻译目标和 Chatbox 发送策略。
- 将麦克风最终字幕和译文发送到 VRChat OSC Chatbox，支持快速输入、翻译预览和多语言发送。启用 OSCQuery 静音同步后，静音或状态未知时停止自动发送。
- SteamVR 头显字幕与手腕对话视图，支持原文、译文和多语言显示。可在仪表盘中调整语言、字幕位置、尺寸、透明度和 Chatbox 设置。
- 桌面 OCR：VRChat 位于前台时，使用可自定义的快捷键框选文字，结果窗口显示原文与多语言译文，支持复制和重新识别。
- VR OCR：通过手势或控制器选择文字区域，译文可显示在手腕面板或原文位置的双眼叠加层。
- OCR 支持 PaddleOCR 云端和本地 PP-OCRv6；本地模型可在设置中下载，使用 CPU 或 DirectML GPU。
- 导入 Yomitan 词典包，在字幕中查词或“问 AI”，收集学习素材并进行词义解释、句型分析和会话回顾。编辑词汇、句型或完形填空卡草稿，通过 AnkiConnect 制卡。

## 隐私

VRCS 不保存原始音频，字幕历史、学习项目、词典和设置默认保存在本机。本地 Qwen ASR 与本地 OCR 在设备上处理语音和图像；云端识别将语音片段或框选图像发送给所选服务商。

翻译、学习分析和“问 AI”会将相关文本、明确选择的上下文及提交的问题发送给配置的服务。本地识别后的文本是否发送到云端，取决于所选翻译和 AI 服务。

## 开发

需要 Windows 10 / 11、Node.js 24+、Rustup（版本和组件见 `rust-toolchain.toml`）、Visual Studio Build Tools 的“使用 C++ 的桌面开发”工作负载，以及已加入 `PATH` 的 CMake。

在仓库根目录的 PowerShell 中运行：

```powershell
npm install
npm run dev
```

默认构建使用 CPU 运行本地 Qwen ASR。Vulkan 加速使用 `npm run dev:vulkan`，命令会自动准备加载器，无需 GPU SDK 或着色器编译器。Qwen 运行组件和模型在识别设置中下载。独立后端开发见 [Core README](core/README.md)。

按改动范围运行检查：

```powershell
npm run check:i18n
npm --workspace apps/desktop test
npm run build:frontend
.\scripts\check-rust.ps1
```

Rust 脚本对两个 crate 执行格式检查、Clippy 和测试；Vulkan 相关改动加上 `-Vulkan`。

构建支持 CPU 和 Vulkan 的 Windows 安装包：

```powershell
npm run build
```

贡献指南见 [CONTRIBUTING.md](CONTRIBUTING.md)，界面翻译指南见 [LOCALIZATION.md](LOCALIZATION.md)。

## 许可证

[GNU Affero General Public License v3.0](LICENSE)（`AGPL-3.0-only`）。第三方许可见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
