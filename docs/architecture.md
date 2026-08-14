# 架构

VRCS 是单进程桌面应用：Tauri 主进程直接嵌入 Rust Core，React 前端仍通过本机 HTTP 与 WebSocket 使用后端能力。该边界让 Core 可以单独运行和测试，同时避免发布 Python 解释器或 Sidecar。

## 组件

### Rust Core（`core/`）

Core 负责音频采集、语音识别、字幕存储和词典查询：

- **音频采集**：WASAPI 系统回环、VRChat 进程专用回环与麦克风输入
- **翻译**：独立翻译 Provider 层负责 DeepL、Microsoft Translator 与 LLM 翻译；通用 LLM 客户端独立于翻译任务，可供后续语法分析等学习功能复用
- **人声切分**：Silero VAD（ONNX），模型不可用时回退到能量检测
- **转写**：Qwen、Fun-ASR、OpenAI 云端流式识别，以及 whisper.cpp CPU/CUDA 本地推理；GGML 模型按需下载
- **存储**：SQLite 保存字幕历史与词典，JSON 文件保存服务配置
- **词典**：内置英日测试词典，支持导入 Yomitan 格式词典包
- **Anki**：连接本机 AnkiConnect 创建笔记
- **OSC Chatbox**：统一格式化、限制长度并调度麦克风最终字幕和手动消息；两类消息共用限速器、UDP 传输与 VRChat 静音安全门，手动发送等待实际传输结果
- **VRChat Mute Sync**：通过本机 OSCQuery 发现 VRChat、启动时读取 `MuteSelf` 并持续同步；静音时停止麦克风管线并丢弃未完成结果，取消静音后按原捕获意图自动重启

`src/lib.rs` 提供嵌入式启动接口和可控生命周期，`src/main.rs` 提供独立调试入口。Windows 音频采集直接在 Core 内完成，不再需要音频辅助进程。

### 桌面端（`apps/desktop/`）

React + TypeScript + Vite 前端打包进 Tauri 2 应用：

- 实时字幕、历史对话、设置页（常规/音频/识别/词典/Anki/Debug）
- 底栏上方的 Chatbox 工作台，支持输入、翻译预览与编辑、格式选择和长度处理
- 选词查释义弹出面板与 Anki 制卡
- 置顶 compact 小窗、系统托盘、开机自启和单实例运行
- 启动时创建 Rust Core，退出时发送优雅停止信号

## 通信

| 通道 | 用途 |
|---|---|
| HTTP REST | 健康检查、设置、设备、词典、模型、Anki、查词制卡与 Chatbox 工作台 |
| WebSocket `/ws` | 实时字幕推送 |
| OSCQuery / mDNS（本机） | 发现 VRChat 并读取麦克风静音状态 |
| UDP OSC `127.0.0.1:9000` | 向 VRChat Chatbox 发送自己的麦克风字幕与译文 |

开发模式固定监听 `127.0.0.1:8766`，避开 AnkiConnect 的 8765 与 VRChat 的 OSC 端口 9000/9001。安装版每次启动选择随机本机端口并使用会话令牌，连接信息由 Tauri 的 `core_connection` 命令提供给前端。

### API 错误契约

Core 的非 2xx HTTP 响应统一返回 `code`、`params` 和 `detail`。`code` 是供客户端分支和本地化使用的稳定语义标识，`params` 只包含可插值的结构化数据，`detail` 保留面向诊断的原始信息；客户端不得通过匹配 `detail` 文案决定业务行为。Anki 状态接口同样提供 `status_code`、`params` 和 `detail`，并暂时保留旧的 `error_code` 与 `message` 字段以兼容现有界面。

## 数据位置

- 桌面端：`%LOCALAPPDATA%\.vrcs`（配置、数据库和模型），升级应用不会覆盖
- 独立 Core：配置路径由 `VRCS_CONFIG` 指定，默认是当前目录的 `config.json`
