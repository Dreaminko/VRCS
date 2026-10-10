<p align="center">
    <img src="apps/desktop/public/logos/VRCS_Logo.svg" width="50%" alt="logo" />
</p>

# VRCS

**English** | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-Hant.md) | [日本語](README.ja-JP.md)

![](./screenshots/01.png)

VRCS is a Windows real-time subtitle, translation, and language-learning tool for VRChat. Capture system audio and microphone input for desktop or SteamVR subtitles, translate text in images with OCR, and use subtitles for dictionary lookup, learning analysis, and Anki cards.

[Download](https://github.com/Dreaminko/VRCS/releases/latest) · [Report an issue](https://github.com/Dreaminko/VRCS/issues) · [Contribute](CONTRIBUTING.md) · [Discord](https://discord.gg/53H872eYq) · [QQ Group](https://qm.qq.com/q/i9kOOxFn44)

## Installation

Download `VRCS-<version>-windows-x64.exe` from [GitHub Releases](https://github.com/Dreaminko/VRCS/releases).

- Windows 10 or 11 with [Microsoft Visual C++ v14 Redistributable (x64)](https://aka.ms/vs/17/release/vc_redist.x64.exe).
- Local Qwen ASR supports CPU or Vulkan. Download its runtime and model in recognition settings.
- An internet connection is needed for the initial Silero VAD download and, if enabled, Smart Turn semantic endpointing.
- Cloud recognition, translation, and learning analysis require the selected provider's API credentials. Provider charges may apply.
- For OCR, download local models or configure a PaddleOCR cloud access token.

## Getting started

1. Follow the setup wizard to select a language and local or cloud recognition.
2. Select system audio, VRChat process audio, or microphone input, then test the microphone and adjust the voice activation threshold.
3. Start transcription. Enable translation, Chatbox output, or the SteamVR overlay as needed.

To run the wizard again, open **Settings › System**. For cloud recognition, see the [Alibaba Cloud free-quota guide (Chinese)](./docs/AlibabaCloud_Free.md).

Open **Settings › System › Features** to turn off optional features. The feature stops and its settings and actions disappear. Your configuration and data are kept and restored when you turn it back on.

## Features

- Real-time subtitles from system or VRChat process audio alongside microphone input, with separate controls for the two audio streams, compact window mode, and local session history.
- Local Qwen3 ASR on CPU or Vulkan, plus cloud recognition with Qwen3 ASR, Fun-ASR, OpenAI Realtime, Gemini, and Groq. Silero VAD and optional Smart Turn control speech segmentation.
- Manual or automatic translation with DeepL, Microsoft Translator, OpenAI, Gemini, Alibaba Cloud LLM, and OpenAI-compatible services. Configure up to three target languages per audio stream, each with its own service and model.
- Qwen Live Translate, Gemini Live Translate (Preview), and OpenAI Realtime Translation produce live source text and translations for each stream's first target language. Qwen also distinguishes speakers.
- Custom prompts, local glossaries, online glossary subscriptions, recent subtitle context, and optional world and member information from [VRCX-0](https://vrcx-0.dev/). Save language presets to switch recognition language, translation targets, and Chatbox sending strategies.
- Send final microphone subtitles and translations to the VRChat OSC Chatbox, with quick input, translation preview, and multilingual output. With OSCQuery mute synchronization enabled, automatic sending stops when muted or when mute status is unknown.
- SteamVR headset subtitles and a wrist conversation view, with source text, translations, and multilingual display. Adjust languages, subtitle position, size, opacity, and Chatbox settings from the dashboard.
- Desktop OCR: when VRChat is in the foreground, use a configurable shortcut to select a text region. View source text and multilingual translations in a result window, copy them, or scan again.
- VR OCR: select text with hand gestures or controllers and display translations on a wrist panel or over the source in both eyes.
- PaddleOCR cloud recognition or local PP-OCRv6 for OCR. Download local models in Settings and use CPU or a DirectML GPU.
- Import Yomitan dictionaries, look up subtitle text or Ask AI, collect learning material, and review definitions, sentence patterns, and conversations. Edit vocabulary, sentence-pattern, or cloze card drafts and create cards through AnkiConnect.

## Privacy

VRCS does not store raw audio. Subtitle history, learning items, dictionaries, and settings are stored locally by default. Local Qwen ASR and local OCR process audio and images on the device; cloud recognition sends speech segments or selected images to the chosen provider.

Translation, learning analysis, and Ask AI send relevant text, explicitly selected context, and submitted questions to the configured service. Whether text from local recognition is sent to the cloud depends on the selected translation and AI services.

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
