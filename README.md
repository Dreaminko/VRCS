<p align="center">
    <img src="apps/desktop/public/logos/VRCS_Logo.svg" width="50%" alt="logo" />
</p>

<p align="center">
    VRCS is a subtitle, translation, language learning, and AI tool for VRChat.
</p>

<p align="center">
    <img src="https://img.shields.io/github/v/release/Dreaminko/VRCS" alt="GitHub Release" />
    <img src="https://img.shields.io/github/license/Dreaminko/VRCS" alt="GitHub License" />
    <img src="https://img.shields.io/github/stars/Dreaminko/VRCS?style=flat" alt="GitHub Repo stars" />
</p>

<p align="center">
    <strong>English</strong> | <a href="README.zh-CN.md">简体中文</a> | <a href="README.zh-Hant.md">繁體中文</a> | <a href="README.ja-JP.md">日本語</a>
</p>

<p align="center">
    <a href="https://github.com/Dreaminko/VRCS/releases/latest">Download</a> · <a href="https://github.com/Dreaminko/VRCS/issues">Report an issue</a> · <a href="CONTRIBUTING.md">Contribute</a> · <a href="https://discord.gg/53H872eYq">Discord</a> · <a href="https://qm.qq.com/q/i9kOOxFn44">QQ Group</a>
</p>

![](./screenshots/01.png)

## Installation

Download `VRCS-<version>-windows-x64.exe` from [GitHub Releases](https://github.com/Dreaminko/VRCS/releases).

- Windows 10 or 11 with [Microsoft Visual C++ v14 Redistributable (x64)](https://aka.ms/vs/17/release/vc_redist.x64.exe).
- Local Qwen ASR supports CPU or Vulkan. Download its runtime and model in recognition settings.
- An internet connection is needed for the initial Silero VAD download and, if enabled, Smart Turn semantic endpointing.
- Cloud recognition, translation, and learning analysis require the selected provider's API credentials. Provider charges may apply.
- For OCR, download the local PP-OCRv6 models.

## Getting started

1. Follow the setup wizard to select a language and local or cloud recognition.
2. Select system audio, VRChat process audio, or microphone input, then test the microphone and adjust the voice activation threshold.
3. Start transcription. Enable translation, Chatbox output, or the SteamVR overlay as needed.

To run the wizard again, open **Settings › System**.

Turn off unused features in **Settings › System › Features** to hide their settings and actions while keeping your configuration and data.

## Features

- Capture system or VRChat process audio alongside your microphone for real-time subtitles, with separate audio controls, a compact window, and local session history.
- Generate AI summary titles for completed conversations and search history by title. Configure a model and enable automatic generation in **Settings › System › Conversation management**; it is off by default. A new conversation ends the previous one; stopping transcription only pauses it. Generate or regenerate titles from a history item's menu. Custom names take priority.
- Choose local Qwen3 ASR on CPU or Vulkan, or cloud services including Qwen3 ASR, Fun-ASR, OpenAI Realtime, Gemini, Groq, and OpenRouter. Use Silero VAD and optional Smart Turn for speech segmentation.
- Translate manually or automatically into up to three languages per audio stream. Choose a service and model for each language, with support for DeepL, Microsoft Translator, OpenAI, Gemini, Alibaba Cloud LLM, and OpenAI-compatible services.
- Get live source text and translations with Qwen Live Translate, Gemini Live Translate, or OpenAI Realtime Translation.
- Customize prompts, local and subscribed glossaries, and subtitle context. Add world and member context from [VRCX-0](https://vrcx-0.dev/), and switch recognition, translation, and Chatbox settings with language presets.
- Send microphone subtitles and translations to the VRChat OSC Chatbox, with quick input, translation preview, multilingual output, and optional OSCQuery mute synchronization.
- Read multilingual subtitles in your SteamVR headset or on your wrist. Adjust subtitle layout, languages, and Chatbox settings from the dashboard.
- Select text on your desktop or in VR for OCR translation. Use local PP-OCRv6 on CPU or DirectML GPU; view results in a desktop window, on your wrist, or over the source text in VR.
- Ask questions about selected images on your desktop or in VR. Type or dictate on the desktop; hold the left trigger to speak in VR. Answers appear in a separate window or a wrist panel with Markdown support. OCR is not required.
- Import Yomitan dictionaries, look up words or Ask AI from subtitles, and review definitions, sentence patterns, and conversations. Edit vocabulary, sentence-pattern, or cloze cards and send them to Anki through AnkiConnect.

## Privacy

VRCS does not store raw audio. Subtitle history, learning items, dictionaries, and settings are stored locally by default. Local Qwen ASR and local OCR process audio and images on the device; cloud speech recognition sends speech segments to the chosen provider.

Translation, learning analysis, and Ask AI send relevant text, explicitly selected context, and submitted questions to the configured service. Whether text from local recognition is sent to the cloud depends on the selected translation and AI services.

Conversation titles send some or all of the conversation's original transcript to the selected model service. Charges may apply. Translations, raw audio, and other conversations are excluded. Long transcripts are sampled; existing history is not processed automatically in bulk.

Desktop OCR and visual questions process only the selected region. Visual questions send the selected image and question text to the configured model service; cloud speech recognition also receives the spoken question. Images and answers are not saved to disk or a question history by default. Speech recognition and image requests may incur separate charges.

## Development

Requirements: Windows 10 or 11, Node.js 24+, Rustup with the version and components in `rust-toolchain.toml`, Visual Studio Build Tools with **Desktop development with C++**, and CMake on `PATH`.

Run from the repository root in PowerShell:

```powershell
npm install
npm run dev
```

The default build uses CPU for local Qwen ASR. Use `npm run dev:vulkan` for Vulkan acceleration; the loader is prepared automatically, with no GPU SDK or shader compiler needed. Download the Qwen runtime and model in recognition settings. For standalone backend development, see [Core README](core/README.md).

Run checks relevant to your changes:

```powershell
npm run check:i18n
npm --workspace apps/desktop test
npm run build:frontend
.\scripts\check-rust.ps1
```

The Rust script checks formatting, Clippy, and tests for both crates. Add `-Vulkan` for Vulkan changes.

Build the Windows installer with CPU and Vulkan support:

```powershell
npm run build
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for contribution guidelines and [LOCALIZATION.md](LOCALIZATION.md) for interface translations.

## License

[GNU Affero General Public License v3.0](LICENSE) (`AGPL-3.0-only`). Third-party licenses are listed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
