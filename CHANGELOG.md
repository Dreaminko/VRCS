# Changelog

## 0.2.0 (2026-10-09)

### English

This release adds local Qwen speech recognition, Vulkan acceleration, and text recognition and translation for desktop and SteamVR, with improvements to live translation and subtitle readability.

#### New Features

1. **Local Qwen ASR:** Added built-in Qwen3-ASR-0.6B model package management with download, verification, deletion, and CPU or Vulkan execution, plus support for connecting to a separately hosted local Qwen ASR service.
2. **Vulkan acceleration:** Local Whisper now supports Vulkan in both Windows editions, with automatic device selection that tries available CUDA, Vulkan, and CPU backends in that order.
3. **Qwen 3.8 Live Translate:** Added Qwen Live Translate through Qwen AI and Alibaba Cloud Model Studio, with text-only translation and speaker labels preserved in live subtitles, history, and VR displays.
4. **Qwen translation glossaries:** Qwen Live Translate now uses enabled shared glossaries for term mappings and can preserve original terms when glossary use is enabled only for speech recognition.
5. **Desktop OCR:** Added a configurable global shortcut, defaulting to `Ctrl+Alt+O`, to select text in the foreground VRChat window and show original text and translations in a result window with copy and rescan controls.
6. **VR OCR:** Added cloud PaddleOCR and local PP-OCRv6 recognition, a hand-anchored selection frame controlled by finger poses or both grip buttons, and translated text placed over the estimated source plane in both eyes.
7. **OCR wrist reader:** Added a separate wrist panel for original text and translations, with its own hand, position, rotation, size, font, and opacity settings.
8. **SteamVR quick settings:** Added a VRCS dashboard panel for display and OCR controls, recognition and translation languages, language presets, OSC options, and overlay position and rotation.

#### Fixes & Improvements

1. **Live translation alignment:** Source and translated text are now aligned locally while translations appear as they arrive, with better handling of delayed output, pending source text, session completion, and source speech already in the target language.
2. **Live translation sessions:** Long silent periods now suspend live translation sessions, and switching recognition providers or applying provider defaults preserves live translation settings.
3. **Long subtitle readability:** Improved Qwen previews and compact subtitles, kept headset text at a stable font size with configurable lines per language, and improved overflow scrolling and speaker-label visibility in VR.
4. **Subtitle display controls:** Fixed conversation auto-scroll after layout changes and made VR partial-recognition and partial-translation switches control the corresponding previews consistently.

#### Notes

1. **Local models and GPU support:** Download the local Qwen or OCR models before use; both installers include the Qwen runtime and Vulkan loader, Vulkan requires a compatible GPU and driver, and the CUDA edition adds CUDA acceleration only for Whisper.
2. **OCR setup and placement:** Enable desktop and VR OCR separately in Settings; cloud OCR requires a PaddleOCR access token, local OCR keeps image recognition on the device while translation uses the selected service, and VR placement is a snapshot with estimated depth that requires rescanning when the source moves.
3. **Live Translate scope:** Live Translate requires automatic translation and applies to the first target language of each audio source; custom text prompts and thinking settings do not apply, shared glossaries are supported by Qwen, and Gemini and OpenAI may generate and bill translated audio that VRCS does not play.

---

### 简体中文

本次更新加入本地 Qwen 语音识别、Vulkan 加速，以及桌面和 SteamVR 文字识别翻译，并改进实时翻译与字幕可读性。

#### 新功能

1. **本地 Qwen ASR：** 新增内置 Qwen3-ASR-0.6B 模型包管理，支持下载、校验、删除及 CPU 或 Vulkan 运行，也可连接单独部署的本地 Qwen ASR 服务。
2. **Vulkan 加速：** 两种 Windows 版本的本地 Whisper 均支持 Vulkan，自动设备选择会依次尝试可用的 CUDA、Vulkan 和 CPU 后端。
3. **Qwen 3.8 Live Translate：** 新增通过 Qwen AI 和阿里云百炼使用 Qwen 实时翻译，支持纯文本译文，并在实时字幕、历史记录和 VR 显示中保留说话人标签。
4. **Qwen 翻译术语表：** Qwen Live Translate 可使用已启用的共享术语表进行术语映射，仅启用语音识别用途时可保留术语原文。
5. **桌面 OCR：** 新增可自定义的全局快捷键，默认为 `Ctrl+Alt+O`，可在前台 VRChat 窗口中框选文字，并在结果窗口查看原文与译文、复制结果或重新框选。
6. **VR OCR：** 新增云端 PaddleOCR 和本地 PP-OCRv6 识别，支持通过手指姿势或双手握持键控制随手移动的选区框，并在双眼视图中将译文覆盖到估算的原文字平面上。
7. **OCR 腕部阅读面板：** 新增独立的原文与译文腕部面板，可单独设置佩戴手、位置、旋转、大小、字号和透明度。
8. **SteamVR 快捷设置：** 新增 VRCS 仪表板面板，可在 VR 内调整显示与 OCR、识别与翻译语言、语言预设、OSC 选项，以及浮层位置和旋转。

#### 修复与改进

1. **实时翻译对齐：** 原文与译文改为在本地对齐，译文到达时即可预览，并改进延迟输出、待对齐原文、会话结束和原语音已使用目标语言时的处理。
2. **实时翻译会话：** 长时间静音时会暂停实时翻译会话，切换识别服务或应用服务默认值时会保留实时翻译设置。
3. **长字幕可读性：** 改进 Qwen 预览和紧凑模式字幕，头显字幕保持稳定字号并可设置每种语言的行数，同时改进 VR 中的溢出滚动和说话人标签显示。
4. **字幕显示控制：** 修复布局变化后会话自动滚动失效的问题，并使 VR 临时识别和临时翻译开关一致地控制对应预览。

#### 使用说明

1. **本地模型与 GPU 支持：** 使用前需下载本地 Qwen 或 OCR 模型；两种安装包均包含 Qwen 运行时和 Vulkan 加载器，Vulkan 需要兼容的 GPU 与驱动，CUDA 版仅为 Whisper 增加 CUDA 加速。
2. **OCR 设置与定位：** 桌面和 VR OCR 需在设置中分别启用；云端 OCR 需要 PaddleOCR 访问令牌，本地 OCR 在设备上识别图像、翻译使用所选服务，VR 定位属于深度估算的快照，原文字移动后需重新扫描。
3. **Live Translate 适用范围：** 需要开启自动翻译，仅作用于每路音源的第一目标语言；自定义文本提示词和思考设置不适用，Qwen 支持共享术语表，Gemini 和 OpenAI 可能生成译后音频并计费，但 VRCS 不会播放这些音频。

**Full Changelog / 完整变更：** [0.1.11...0.2.0](https://github.com/Dreaminko/VRCS/compare/0.1.11...0.2.0)
