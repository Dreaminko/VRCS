# VRCS

VRCS 是一个本地优先的 VRChat 字幕学习工具。它同时捕获 Windows 系统输出和选定的麦克风，用 Silero VAD 切分人声，再交给 faster-whisper 转写。字幕会实时显示在桌面端，也可以选词查释义并发送到本机 Anki。

> A local-first VRChat subtitle mining tool that captures system audio, transcribes conversations with faster-whisper, provides instant dictionary lookup, and creates Anki cards from real conversation context.

## 项目状态

当前为 0.1 基础实现。系统音频、VAD、Whisper、字幕存储与实时推送链路已经打通，桌面端和扩展接口可用；发布安装包和 VR Overlay 仍在后续路线图中。

## 当前功能

- Windows WASAPI 系统回环与麦克风双路捕获
- Silero VAD 人声切分，开发环境缺少可选依赖时使用能量检测回退
- faster-whisper 本地转写，支持模型、语言、设备和计算类型设置
- FastAPI HTTP 接口和 WebSocket 字幕推送
- SQLite 字幕历史与内置英日测试词典
- React + Tauri 桌面端，包括实时字幕、历史、识别设置和音频设备页面
- 通过本机 AnkiConnect 创建基础卡片

VR Overlay、说话人分离、翻译和完整词典导入还没有实现，见 [路线图](docs/roadmap.md)。

## 技术栈

Core 服务使用 Python 3.11 或 3.12、FastAPI、SQLite、PyAudioWPatch、Silero VAD 和 faster-whisper。桌面端使用 TypeScript、React、Vite 与 Tauri 2。

## 安装

需要 Windows 10 或 11、Python 3.11 或 3.12、Node.js 20+、Rust stable 和 Tauri 的 Windows 构建依赖。首次加载 Whisper 模型时需要联网下载模型文件。

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

打开第一个终端启动 Core 服务：

```powershell
cd core-python
.venv\Scripts\Activate.ps1
python -m app.main
```

打开第二个终端启动桌面端：

```powershell
npm run dev
```

Core 默认监听 `http://127.0.0.1:8765`，WebSocket 地址是 `ws://127.0.0.1:8765/ws`。配置首次启动时写入 `core-python/config.json`，SQLite 数据保存在 `core-python/data/vrcs.db`。

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
