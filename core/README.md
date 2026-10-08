# VRCS Core（Rust）

VRCS 的本地后端。数据面、音频采集、VAD、本地/云端 ASR 与字幕发布管线均使用 Rust 实现，并作为库直接嵌入 Tauri 主进程；也可单独运行二进制调试 API。

## 当前状态

| 能力 | 状态 |
|---|---|
| 配置读写、schema v1→v11 迁移 | 已实现（`src/config.rs`） |
| 字幕历史存储与裁剪（SQLite） | 已实现（`src/db.rs`） |
| 词典查询、Yomitan 词典包导入/删除 | 已实现（`src/db.rs`、`src/yomitan.rs`） |
| AnkiConnect 状态探测与制卡 | 已实现（`src/anki.rs`） |
| HTTP API、WebSocket 字幕推送、Bearer 鉴权、CORS | 已实现（`src/server.rs`） |
| 音频设备枚举（回环+麦克风）与设置校验 | 已实现（`src/audio.rs`、`/api/audio/devices`） |
| 音频采集（系统回环/进程回环/麦克风，`AudioCapture`） | 已实现并接入管线（`src/audio.rs`） |
| VAD | 已实现 Silero ONNX、能量检测回退与流式语音分段（`src/vad.rs`） |
| ASR | 已实现 whisper.cpp CPU/Vulkan/CUDA、应用管理的本地 Qwen3 ASR，以及云端 Qwen3 ASR / Fun-ASR / OpenAI WebSocket 流式适配（`src/asr.rs`） |
| 识别管线与 `/api/capture/start` `/api/capture/stop` | 已实现双音源采集、VAD 上传门控、增量事件、最终结果 SQLite 写入与 WebSocket 发布（`src/pipeline.rs`） |
| OSC Chatbox / Mute Sync | 已实现麦克风最终字幕与译文的本机 UDP 输出、限长、限速、静音发送门，以及通过 OSCQuery 同步 VRChat 麦克风静音状态（`src/osc.rs`、`src/vrchat_mute_sync.rs`） |

SQLite DDL 与 Python 版完全一致，可直接打开已有的 `vrcs.db`。配置文件格式、环境变量（`VRCS_CONFIG` / `VRCS_HOST` / `VRCS_PORT` / `VRCS_SESSION_TOKEN`）与 Python 版一致。回环监听未设置 token 时会自动生成临时 token；监听非回环地址时必须显式设置非空的 `VRCS_SESSION_TOKEN`。Yomitan 压缩包上限为 128 MiB，并额外限制解压大小、压缩比、单文件大小和词条文本总量。

Core 首次启动时会从 Silero 官方仓库下载固定的 v6.2.1 模型到配置文件同目录的 `models/`。下载文件仅在大小为 2,327,524 字节且 SHA-256 为 `1a153a22f4509e292a94e67d6f9b85e8deb25b4988682b7e174c65279d8788e3` 时安装；已有文件也会在启动和加载时校验。`VRCS_SILERO_MODEL` 可将同一固定版本放在自行管理的位置，此路径不会触发自动下载。模型下载、校验、初始化或推理失败时自动回退到能量检测，`/health` 的 `vad_backend` 和 `vad_model_version` 会报告实际状态。

Whisper GGML 模型默认存放在配置文件同目录的 `models/whisper/`，可通过设置页或配置项 `storage.model_directory` 自定义；相对路径以配置文件目录为基准。修改保存位置时，Core 会自动迁移已下载的有效模型；跨磁盘时采用复制完成后再删除源文件，迁移失败则保留原设置和原目录。`VRCS_ASR_MODEL_DIR` 可用于启动时强制覆盖该设置。`/api/asr/models` 会报告模型大小与下载进度，下载文件完成前使用 `.part` 后缀；下载源固定到已知仓库版本，完成后校验精确大小与 SHA-256。运行前会按文件大小和修改时间复用校验记录，首次发现、文件变化或模型加载失败时重新计算 SHA-256；删除下载中的模型会取消任务。

Core 默认构建包含 GGML Vulkan 后端；`--features cuda` 会额外编译 CUDA 后端，`--no-default-features` 则同时关闭 Whisper 和本地 Qwen 的 Vulkan 加速，两者仅使用 CPU。`local.device=auto` 会按 CUDA、Vulkan、CPU 的顺序尝试可用后端；GPU 模型加载失败时会记录原因并尝试下一后端。显式选择 CUDA 或 Vulkan 时会检查后端与设备是否可用；当前识别后端为本地 Whisper 且所选 GPU 不可用时，配置会恢复自动选择模式。`/api/asr/capabilities` 返回 CUDA 和 Vulkan 的可用状态与设备数量。

应用管理的本地 Qwen3 ASR 使用独立的 `llama-server` 运行时，支持 CPU 或 Vulkan，与 Whisper 共用 `vulkan` Cargo 功能开关。启用该功能时，自动设备模式会优先使用可用的 Vulkan GPU；GPU 启动失败且启动时限仍有剩余时会尝试 CPU。关闭该功能时，不探测 GPU，自动设备模式使用 CPU，显式选择 GPU 会报告 Vulkan 后端不可用。模型包由应用负责下载和完整性校验，语音在本机处理。

## 运行与测试

在 Windows 上安装 Rustup、Visual Studio C++ Build Tools，并将 CMake 和 Ninja 加入 `PATH`。Rustup 会使用仓库根目录 `rust-toolchain.toml` 指定的版本和组件。默认 Vulkan 构建还需要 SDK；从仓库根目录的 PowerShell 开始：

```powershell
& .\scripts\prepare-vulkan-sdk.ps1
cd core
cargo test
cargo run
```

此时 Core 监听 `127.0.0.1:8766`，配置写入 `core/config.json`。准备脚本会复用 `VULKAN_SDK` 指向且包含 `Bin/glslc.exe` 的 SDK，否则下载并校验固定版本 `1.4.309.0`，放入 `core/.cache/vulkan-sdk/1.4.309.0`。脚本同时准备 Vulkan 加载器，并在当前终端设置 `VULKAN_SDK`、`PATH` 和 `CMAKE_GENERATOR=Ninja`。新开终端后需重新执行准备脚本。

若要使用应用管理的本地 Qwen，先在仓库根目录准备其运行时；桌面开发和发布命令会自动执行此步骤：

```powershell
& .\scripts\prepare-qwen-runtime.ps1
```

CUDA Toolkit 已安装并设置 `CUDA_PATH` 时，在完成 SDK 准备后从仓库根目录运行：

```powershell
npm run dev:core:cuda
```

该命令还会将 CUDA 运行时目录加入进程的 `PATH`。若 Whisper 和本地 Qwen 均仅使用 CPU，可在仓库根目录运行，无需准备 Vulkan SDK：

```powershell
cargo run --manifest-path core/Cargo.toml --no-default-features
```

对两个 Rust crate 执行格式检查、Clippy 和测试，在仓库根目录运行：

```powershell
.\scripts\check-rust.ps1
```

## 音频实现要点

- `AudioCapture`（`src/audio.rs`）替代 Python 版 PyAudioWPatch 封装与 `vrcs-process-audio` 子进程：采集线程内完成 COM 初始化、事件驱动读取、原生 PCM/Float→f32、多声道混平均与线性插值重采样，按 512 采样块经 channel 交给下游。
- 系统回环/麦克风按**设备混音格式**打开（共享模式回环不支持 autoconvert，已实测验证）；进程回环保留 autoconvert 直接请求 16 kHz 单声道（原 helper 方案）。
- 设备 id 为 WASAPI 端点 ID 的 FNV-1a 散列，跨重启稳定（Python 版的 PortAudio 序号重启后可能变化）。**迁移注意**：Python 时代配置里存的数值型 device_id 在 Rust Core 下会校验失败，用户在设置页重新选择一次即可。
- `GetNextPacketSize == 0` 表示无排队包（wasapi 映射为 `Some(0)`），drain 循环必须显式退出。

## 集成方式

- `src/lib.rs` 暴露 `start(CoreOptions)` 与 `CoreHandle`，由 Tauri 负责启动和优雅停止。
- `src/main.rs` 是轻量独立入口，便于只调试 Core API。
- 桌面端和 Core 保持现有 HTTP/WebSocket 边界，前端无需感知后端已改为进程内运行。
