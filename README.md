# VRCS

VRCS 是一个本地优先的 VRChat 字幕学习工具。它同时捕获 Windows 系统输出和选定的麦克风，用 Silero VAD 切分人声，再交给 faster-whisper 转写。字幕会实时显示在桌面端，也可以选词查释义并发送到本机 Anki。

> A local-first VRChat subtitle mining tool that captures system audio, transcribes conversations with faster-whisper, provides instant dictionary lookup, and creates Anki cards from real conversation context.

## 项目状态

当前为 0.1 基础实现。系统音频、VAD、Whisper、字幕存储与实时推送链路已经打通，桌面端、扩展接口和 Windows x64 发布安装包可用；VR Overlay 仍在后续路线图中。

## 当前功能

- Windows WASAPI 系统回环、VRChat 进程专用回环与麦克风双路捕获
- Silero VAD 人声切分，开发环境缺少可选依赖时使用能量检测回退
- faster-whisper 本地转写，支持模型、语言、设备和计算类型设置
- FastAPI HTTP 接口和 WebSocket 字幕推送
- SQLite 字幕历史与内置英日测试词典
- React + Tauri 桌面端，包括实时字幕、历史、识别设置和音频设备页面
- 通过本机 AnkiConnect 创建基础卡片

VR Overlay、说话人分离、翻译和完整词典导入还没有实现，见 [路线图](docs/roadmap.md)。

## 技术栈

Core 服务使用 Python 3.11 或 3.12、FastAPI、SQLite、PyAudioWPatch、Silero VAD 和 faster-whisper。桌面端使用 TypeScript、React、Vite 与 Tauri 2。

## 运行 Release 安装版

Release 安装包面向 Windows 10/11 x64。运行安装版需要：

- **Microsoft Visual C++ v14 Redistributable（x64）**：必须安装[微软当前支持的最新 x64 版本](https://aka.ms/vc14/vc_redist.x64.exe)。不要固定安装旧版 14.29；微软要求目标机器上的运行库版本不低于构建应用所用的 MSVC 工具链版本。版本与兼容性说明见[微软官方文档](https://learn.microsoft.com/cpp/windows/latest-supported-vc-redist)。
- **Microsoft Edge WebView2 Evergreen Runtime**：桌面界面需要。Windows 11 和已更新的 Windows 10 通常已经包含；VRCS 安装器内置 WebView2 bootstrapper，在缺失时会联网安装。也可以从[微软 WebView2 页面](https://developer.microsoft.com/microsoft-edge/webview2/)手动安装。
- **64 位、支持 SSE 4.1 的处理器**：CTranslate2 的 Windows x64 预编译运行库最低要求 SSE 4.1，并会自动选择 AVX、AVX2 或 AVX512 优化路径。详见[CTranslate2 硬件支持](https://opennmt.net/CTranslate2/hardware_support.html)。
- **网络连接（首次识别模型）**：标准安装包不包含 Whisper 模型，首次使用所选模型时需要下载。模型之后缓存在 `%LOCALAPPDATA%\.vrcs\models`。

默认 CPU 模式不需要 CUDA。只有在识别设置中选择 NVIDIA GPU/CUDA 时，才需要兼容的 NVIDIA 显卡与驱动、CUDA 12 的 cuBLAS 以及 CUDA 12 的 cuDNN 9；具体要求见[faster-whisper 官方说明](https://github.com/SYSTRAN/faster-whisper#gpu)。Anki 和 AnkiConnect 仅在使用制卡功能时需要。

安装版已经包含 Python 解释器、VRCS Core、ONNX Runtime、CTranslate2、音频组件和前端资源，不需要另外安装 Python、Node.js、Rust 或 FFmpeg。VRCS 只向 faster-whisper 传递内存中的 PCM 音频，因此 Release 不携带用于媒体文件解码的 PyAV/FFmpeg，也不携带 PyTorch。

## 从源码开发

开发环境需要 Windows 10 或 11、Python 3.11 或 3.12、Node.js 20+、Rust stable 和 Tauri 的 Windows 构建依赖。首次加载 Whisper 模型时需要联网下载模型文件。

“仅采集 VRChat 音频”使用 Windows 进程回环 API，需要 Windows 10 Build 20348 或更高版本。开启后请先运行 VRChat；VRCS 会自动定位 `VRChat.exe`，并隐藏系统输出设备选择。

```powershell
cd core-python
py -3.12 -m venv .venv
.venv\Scripts\Activate.ps1
pip install -e ".[full,dev]"

cd ..
npm install
```

如果只想调试 API，可以安装轻量依赖：

```powershell
cd core-python
pip install -e ".[dev]"
```

此时 `/health`、设置、字幕历史和测试词典可以运行，音频与转写接口会返回缺少可选依赖的明确错误。

## 开发启动

在仓库根目录运行：

```powershell
npm run dev
```

该命令会同时启动 Core 服务与 Tauri 桌面端；按 `Ctrl+C` 会一起停止。需要单独调试时，可以分别运行 `npm run dev:core` 或 `npm run dev:desktop`。

开发模式下 Core 默认监听 `http://127.0.0.1:8765`，WebSocket 地址是 `ws://127.0.0.1:8765/ws`。源码运行时配置写入 `core-python/config.json`，SQLite 数据保存在 `core-python/data/vrcs.db`。

安装版由 Tauri 自动启动 Core，并为每次会话分配随机本机端口。安装版的配置、数据库、Whisper 模型和日志统一保存在 `%LOCALAPPDATA%\.vrcs`，升级应用不会覆盖这些数据。

## 构建 Windows Release

Release 目标为 Windows 10/11 x64。构建机需要 Python 3.12 x64、Node.js 20+、Rust stable，以及安装了“使用 C++ 的桌面开发”工作负载的 Visual Studio Build Tools。构建脚本会自动创建独立的 Python 3.12 环境，把 Core 冻结为 Tauri Sidecar，并生成包含前端和后端的 NSIS 安装包：

```powershell
npm run build
```

完整 Release 构建会重新创建 `core-python/.venv-release`，确保冻结环境不残留已经移除的传递依赖；Python 测试使用独立的 `core-python/.venv-test`。Sidecar 构建完成后还会执行安装态自检，验证模型下载、CTranslate2、WASAPI 和 Silero ONNX 运行路径。

也可以显式校验版本：

```powershell
.\scripts\build-release.ps1 -Version 0.1.0
```

安装包和 SHA-256 校验文件输出到 `apps/desktop/src-tauri/target/release/bundle/nsis/`。构建不会把构建机的 Visual C++ DLL 复制到应用目录；目标机器必须安装上面列出的最新 Microsoft Visual C++ v14 x64 Redistributable。

为控制分发体积，Release 只收集运行时实际使用的 faster-whisper VAD 模型与 CTranslate2 原生库，并排除 PyTorch、Torchaudio、PyAV/FFmpeg、旧版 Silero Python 包、Hugging Face Xet、开发测试和热重载组件。Whisper 模型仍按需下载，不会进入安装包。

推送与 `tauri.conf.json` 版本一致的标签（例如 `v0.1.0`）会触发 Windows GitHub Actions，并创建包含安装包和校验文件的 Draft Release。公开发布前建议为 Sidecar、主程序和安装器配置 Windows Authenticode 代码签名；未签名安装包可以运行，但可能触发 SmartScreen 警告。

## AnkiConnect

在 Anki 中安装 AnkiConnect 插件并保持 Anki 运行。VRCS 会向 `http://127.0.0.1:8766` 发送 `addNote` 请求，默认使用 `VRCS` 牌组和 `Basic` 笔记类型。请提前创建同名牌组；卡片字段需要包含 `Front` 和 `Back`。

桌面端选中字幕文字后会打开查词面板。内置词典只含少量测试词条，查到释义后可以点击“添加到 Anki”。

## 隐私

音频、转写和存储都在本机完成。VRCS 默认不保存原始音频，也不会上传音频。Anki 制卡只连接本机 AnkiConnect。录制他人语音前，请确认符合 VRChat 社区规则和当地法律。完整说明见 [隐私文档](docs/privacy.md)。

## 项目文档

- [架构](docs/architecture.md)
- [开发说明](docs/development.md)
- [隐私说明](docs/privacy.md)
- [路线图](docs/roadmap.md)

## License

[MIT](LICENSE)
