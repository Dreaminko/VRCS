# VRCS Core（Rust）

VRCS 的本地后端。数据面、音频采集、VAD、本地/云端 ASR 与字幕发布管线均使用 Rust 实现，并作为库直接嵌入 Tauri 主进程；也可单独运行二进制调试 API。

## 当前状态

| 能力 | 状态 |
|---|---|
| 配置读写、schema v1→v29 迁移 | 已实现（`src/config.rs`） |
| 字幕历史存储与裁剪（SQLite） | 已实现（`src/db.rs`） |
| 词典查询、Yomitan 词典包导入/删除 | 已实现（`src/db.rs`、`src/yomitan.rs`） |
| AnkiConnect 状态探测与制卡 | 已实现（`src/anki.rs`） |
| HTTP API、WebSocket 字幕推送、Bearer 鉴权、CORS | 已实现（`src/server.rs`） |
| 音频设备枚举（回环+麦克风）与设置校验 | 已实现（`src/audio.rs`、`/api/audio/devices`） |
| 音频采集（系统回环/进程回环/麦克风，`AudioCapture`） | 已实现并接入管线（`src/audio.rs`） |
| VAD | 已实现 Silero ONNX、能量检测回退与流式语音分段（`src/vad.rs`） |
| ASR | 已实现应用管理的本地 Qwen3 ASR、外部本机 Qwen HTTP 服务，以及云端 Qwen3 ASR / Fun-ASR / OpenAI WebSocket 流式适配（`src/asr.rs`） |
| 识别管线与 `/api/capture/start` `/api/capture/stop` | 已实现双音源采集、VAD 上传门控、增量事件、最终结果 SQLite 写入与 WebSocket 发布（`src/pipeline.rs`） |
| OSC Chatbox / Mute Sync | 已实现麦克风最终字幕与译文的本机 UDP 输出、限长、限速、静音发送门，以及通过 OSCQuery 同步 VRChat 麦克风静音状态（`src/osc.rs`、`src/vrchat_mute_sync.rs`） |

SQLite DDL 与 Python 版完全一致，可直接打开已有的 `vrcs.db`。配置文件格式、环境变量（`VRCS_CONFIG` / `VRCS_HOST` / `VRCS_PORT` / `VRCS_SESSION_TOKEN`）与 Python 版一致。回环监听未设置 token 时会自动生成临时 token；监听非回环地址时必须显式设置非空的 `VRCS_SESSION_TOKEN`。Yomitan 压缩包上限为 128 MiB，并额外限制解压大小、压缩比、单文件大小和词条文本总量。

Core 首次启动时会从 Silero 官方仓库下载固定的 v6.2.1 模型到配置文件同目录的 `models/`。下载文件仅在大小为 2,327,524 字节且 SHA-256 为 `1a153a22f4509e292a94e67d6f9b85e8deb25b4988682b7e174c65279d8788e3` 时安装；已有文件也会在启动和加载时校验。`VRCS_SILERO_MODEL` 可将同一固定版本放在自行管理的位置，此路径不会触发自动下载。模型下载、校验、初始化或推理失败时自动回退到能量检测，`/health` 的 `vad_backend` 和 `vad_model_version` 会报告实际状态。

新配置的 ASR 模型包默认存放在配置文件同目录的 `models/asr/`，可通过设置页或 `storage.model_directory` 自定义；相对路径以配置文件目录为基准。旧配置保留原目录，包括 `models/whisper/` 和其中的 Qwen 包。修改保存位置时，Core 只迁移托管 Qwen 包；目标冲突或迁移失败时保留原设置与原目录。旧 Whisper 文件和其他非托管文件留在原目录，不会自动删除。`VRCS_ASR_MODEL_DIR` 可在启动时覆盖该设置。

Qwen 模型包通过 `/api/asr/local-models/qwen` 管理，下载源固定到已知仓库版本，完成后校验大小与 SHA-256。文件未变化时复用校验记录；删除正在下载的包会取消任务，使用中的包受到删除保护。

Core 默认构建和 `--no-default-features` 均支持云端功能和托管 Qwen CPU 识别。`--features vulkan` 允许独立 `llama-server` 运行时使用 Vulkan GPU，无需 Vulkan SDK 或着色器编译器。自动设备模式优先使用可用 GPU；GPU 启动失败且启动时限仍有剩余时尝试 CPU。未启用 Vulkan 时不探测 GPU，自动模式使用 CPU，显式选择 GPU 会报告 Vulkan 后端不可用。语音在本机处理。

升级配置时，旧本地 Whisper 选择迁移为托管 Qwen；已有 Qwen 设置、目录及校验记录保留。缺少 Qwen 模型或运行时时，需要先安装再开始识别。云端连接失败继续重连，不再回退到 Whisper。远程服务的 Whisper 模型仍可使用。

Windows 构建的本地 PP-OCRv6 small 支持 ONNX Runtime DirectML，无需 CUDA 或 Vulkan SDK。`ocr.device` 默认为 `cpu`，设为 `directml` 时会优先选择高性能兼容 GPU；模型加载或推理失败后回退 CPU。回退后的 CPU 会话保持使用，直到切换推理设备或重新启动服务。`GET /api/ocr/runtime` 返回最近请求的设备、实际加载的设备和回退原因。其他平台会回退 CPU。

## 运行与测试

在 Windows 上安装 Rustup、Visual Studio C++ Build Tools，并将 CMake 加入 `PATH`。Rustup 会使用仓库根目录 `rust-toolchain.toml` 指定的版本和组件。CPU 和 Vulkan 构建均无需 GPU SDK。从仓库根目录的 PowerShell 开始：

```powershell
cd core
cargo test
cargo run
```

此时 Core 监听 `127.0.0.1:8766`，配置写入 `core/config.json`。

若要启用 Vulkan，从仓库根目录运行：

```powershell
& .\scripts\prepare-vulkan-runtime.ps1
cargo test --manifest-path core/Cargo.toml --features vulkan
cargo run --manifest-path core/Cargo.toml --features vulkan
```

准备脚本只下载并校验固定版本的 Vulkan loader，将其暂存到应用资源和本地构建目录，不安装 Vulkan SDK 或修改系统 `PATH`。使用 GPU 还需要兼容的显卡和驱动。

若要使用托管本地 Qwen，先在仓库根目录准备其运行时，并安装模型包；桌面应用会通过识别设置按需下载：

```powershell
& .\scripts\prepare-qwen-runtime.ps1
```

仅使用 CPU 时可使用默认构建，或显式运行：

```powershell
cargo run --manifest-path core/Cargo.toml --no-default-features
```

对两个 Rust crate 执行格式检查、Clippy 和测试，在仓库根目录运行：

```powershell
.\scripts\check-rust.ps1
.\scripts\check-rust.ps1 -Vulkan
```

可用 `-Check format`、`-Check clippy` 或 `-Check test` 单独执行一个阶段。Vulkan 检查会准备 loader，无需 SDK。发布包支持 CPU/Vulkan。

## 音频实现要点

- `AudioCapture`（`src/audio.rs`）替代 Python 版 PyAudioWPatch 封装与 `vrcs-process-audio` 子进程：采集线程内完成 COM 初始化、事件驱动读取、原生 PCM/Float→f32、多声道混平均与线性插值重采样，按 512 采样块经 channel 交给下游。
- 系统回环/麦克风按**设备混音格式**打开（共享模式回环不支持 autoconvert，已实测验证）；进程回环保留 autoconvert 直接请求 16 kHz 单声道（原 helper 方案）。
- 设备 id 为 WASAPI 端点 ID 的 FNV-1a 散列，跨重启稳定（Python 版的 PortAudio 序号重启后可能变化）。**迁移注意**：Python 时代配置里存的数值型 device_id 在 Rust Core 下会校验失败，用户在设置页重新选择一次即可。
- `GetNextPacketSize == 0` 表示无排队包（wasapi 映射为 `Some(0)`），drain 循环必须显式退出。

## 集成方式

- `src/lib.rs` 暴露 `start(CoreOptions)` 与 `CoreHandle`，由 Tauri 负责启动和优雅停止。
- `src/main.rs` 是轻量独立入口，便于只调试 Core API。
- 桌面端和 Core 保持现有 HTTP/WebSocket 边界，前端无需感知后端已改为进程内运行。
