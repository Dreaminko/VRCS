<p align="center">
    <img src="apps/desktop/public/logos/VRCS_Logo.svg" width="50%" alt="logo" />
</p>

# VRCS

**English** | [简体中文](README.zh-CN.md) | [日本語](README.ja-JP.md)

VRCS is a Windows real-time subtitle and language-learning tool designed for VRChat. It captures system output, VRChat process audio, and microphone input, displays subtitles on the desktop or in SteamVR, and uses those subtitles for translation, dictionary lookup, learning analysis, Anki card creation, and VRChat Chatbox output.

[Download the latest release](https://github.com/Dreaminko/VRCS/releases/latest) · [Report an issue](https://github.com/Dreaminko/VRCS/issues) · [Contribute](CONTRIBUTING.md)

[Discord](https://discord.gg/53H872eYq) · [QQ Group](https://qm.qq.com/q/i9kOOxFn44)

## Download and installation

Download the appropriate installer from [GitHub Releases](https://github.com/Dreaminko/VRCS/releases):

| Installer | Recommended use | Additional requirements |
|---|---|---|
| `VRCS-<version>-windows-x64.exe` | Recommended for most users; supports cloud recognition and local CPU-based Whisper | No CUDA required |
| `VRCS-<version>-windows-x64-CUDA.exe` | Accelerates local Whisper with an NVIDIA GPU | CUDA 12.x Runtime, cuBLAS, and a compatible NVIDIA GPU and driver |

The standard and CUDA editions share the same configuration, database, and model directories, so you can switch between them without migrating data.

Runtime requirements:

- [Microsoft Visual C++ v14 Redistributable (x64)](https://aka.ms/vs/17/release/vc_redist.x64.exe)
- An internet connection on first launch to download the pinned Silero VAD model
- An initial model download when using local Whisper
- API credentials for the selected providers when using cloud recognition, translation, or learning analysis; provider charges may apply

## Getting started
The setup wizard opens on first launch:

1. Select Simplified Chinese, Japanese, English, or the system language.
2. Select cloud-based real-time recognition or local Whisper.
3. Configure system audio, VRChat process audio, and the microphone.
4. Test the microphone and calibrate the voice activation threshold.
5. Complete setup and start transcription.

You can change the configuration at any time in the application. To run the setup wizard again, open **Settings → System**.

## Features

### Real-time subtitles and audio

- Windows WASAPI system loopback, dedicated VRChat process loopback, and microphone capture
- Dual-stream transcription for system audio and microphone input, with independent audio source and device controls
- Silero ONNX VAD with automatic fallback to energy-based detection when the model is unavailable
- Incremental real-time subtitles, final subtitles, session history, and compact window mode
- Local SQLite subtitle history with session organization, renaming, and cleanup

### Speech recognition

- Local `whisper.cpp` with CPU support and optional CUDA acceleration
- Local Whisper model download, integrity verification, migration, and deletion
- Alibaba Cloud Qwen3 ASR and Fun-ASR real-time streaming recognition
- OpenAI Realtime Transcription
- Automatic reconnection for cloud services and configurable failure-handling policies

### Translation and context

- Manual or automatic translation
- DeepL, Microsoft Translator, OpenAI, Gemini, and Alibaba Cloud LLM
- OpenAI-compatible Chat Completions services, including DeepSeek, Groq, OpenRouter, LM Studio, Ollama, and custom endpoints
- Custom system prompts, local glossaries, online glossary subscriptions, and recent subtitle context
- Optional access to the current world name, member display names, and member languages from the local [VRCX-0](https://vrcx-0.dev/) instance to enrich supported ASR or LLM requests with context

### VRChat and SteamVR

- Send final microphone subtitles and translations to the VRChat OSC Chatbox
- Quick Chatbox input, translation preview, formatting, and 144-character handling
- Synchronize VRChat's `MuteSelf` state through OSCQuery and block automatic sending when muted or when the state is unknown
- SteamVR VR Overlay with headset subtitles and a wrist-mounted conversation view
- Configure the overlay to show original text, translations, or both; select system audio, microphone, and Chatbox sources; and adjust position, size, opacity, and display duration

### Dictionary lookup, learning, and Anki

- Import and manage Yomitan dictionary packages
- Select words in subtitles for lookup while preserving the original sentence and translation context
- Collect learning material from live subtitles, subtitle history, and lookup results
- Use the selected LLM for contextual definitions, sentence-pattern analysis, and conversation reviews
- Edit drafts for vocabulary, sentence-pattern, and cloze cards
- Create cards through AnkiConnect after selecting a deck, note type, and field mapping

## Privacy and data

VRCS does not store raw audio. Subtitle history, sessions, learning items, dictionaries, and configuration are stored locally by default.

When using local Whisper, speech is not sent to the cloud. When using cloud recognition, detected speech segments are sent to the selected recognition provider. When using cloud translation or learning analysis, the relevant text and any context explicitly selected by the user are sent to the corresponding provider.

## Run from source

Development requirements:

- Windows 10 or 11
- Node.js 24+
- Rust stable
- Visual Studio Build Tools with the **Desktop development with C++** workload
- NVIDIA CUDA Toolkit with `CUDA_PATH` configured, only for CUDA development

```powershell
npm install
npm run dev
```

The default command uses a CPU build. To enable CUDA:

```powershell
npm run dev:cuda
```

To run only the standalone Rust Core:

```powershell
npm run dev:core
npm run dev:core:cuda
```

The standalone Core listens on `http://127.0.0.1:8766` by default, and its subtitle WebSocket is available at `ws://127.0.0.1:8766/ws`. The desktop application automatically generates and manages a local session token. If you run the Core separately and bind it to a non-loopback address, you must explicitly set a non-empty `VRCS_SESSION_TOKEN`.

## Testing

```powershell
npm run check:i18n
npm --workspace apps/desktop test
npm run build:frontend
.\scripts\test-core.ps1
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
```

## Build a release

Build the standard Windows installer:

```powershell
npm run build
```

Build both the standard and CUDA editions:

```powershell
.\scripts\build-release.ps1 -Version 0.1.0 -IncludeCuda
```

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) before contributing. To add or update an interface language, see [LOCALIZATION.md](LOCALIZATION.md). Run the tests relevant to your changes before submitting, and do not commit generated build artifacts.

## License

VRCS is licensed under the [GNU Affero General Public License v3.0](LICENSE) (`AGPL-3.0-only`). See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for third-party components and their licenses.
