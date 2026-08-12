# 隐私说明

VRCS 的音频采集、字幕存储和词典查询在本机完成。语音识别默认使用 Qwen3 ASR 云端服务，也可切换到 Fun-ASR、OpenAI 或完全本地的 Whisper。

## 音频

- 系统输出、VRChat 进程音频和麦克风输入仅在内存中以 PCM 流的形式处理，用于实时转写。
- VRCS **不保存原始音频**。
- 使用 Qwen、Fun-ASR 或 OpenAI 云端识别时，检测到语音的 PCM 片段会通过加密连接发送给所选服务商；使用本地 Whisper 时，音频不会发送给云端 ASR 服务。

## 网络访问

VRCS 只在以下功能需要时访问外部网络或本机网络服务：

- **Whisper 模型下载**：首次使用某个识别模型时从 Hugging Face 下载，之后缓存于本地（安装版为 `%LOCALAPPDATA%\.vrcs\models`），后续使用离线可用。
- **Silero VAD 模型下载**：首次启动时从 Silero 官方 GitHub 仓库下载固定版本，校验文件大小和 SHA-256 后缓存于本地。
- **云端语音识别**：选择 Qwen、Fun-ASR 或 OpenAI 时，实时发送检测到的语音片段并接收增量转写结果。服务商如何处理这些数据由对应服务条款与隐私政策约束。
- **字幕翻译与模型查询**：启用翻译、测试 API 配置或查询兼容服务的模型列表时，会连接用户选择的 DeepL、Microsoft Translator、OpenAI、Alibaba Cloud 或 OpenAI 兼容服务。
- **WebView2 运行时**：安装器在目标机器缺少 WebView2 时会联网安装（微软官方 bootstrapper）。
- **AnkiConnect**：制卡和连接检查只连接本机 AnkiConnect，默认地址为 `http://127.0.0.1:8765`。
- **OSC Chatbox**：用户启用该功能后，自己的麦克风识别文本和译文会通过本机 UDP 发送给 VRChat，并可能作为聊天框内容显示给附近玩家。关闭 OSC 输出时不会发送。

除上述情况外，查词和历史记录均不产生网络请求。

## 本地数据

- 字幕文本历史保存在本机 SQLite 数据库（桌面端：`%LOCALAPPDATA%\.vrcs\data\vrcs.db`；独立运行 Core 时相对于其配置文件保存）。
- 设置与桌面偏好保存在本机配置文件与应用数据目录中。
- 卸载应用不会自动删除 `%LOCALAPPDATA%\.vrcs` 中的数据，需要时可手动删除。

## Anki

制卡功能只连接本机的 AnkiConnect（默认 `http://127.0.0.1:8765`），发送的内容限于词条、读音、释义、语境和语言等笔记字段，不包含音频。

启用字幕翻译后，字幕文本会发送到用户选择的 DeepL、Microsoft Translator、OpenAI 或 Alibaba Cloud API。关闭翻译时不会发送；API Key 保存在系统凭据管理器或由环境变量提供，不写入配置文件或字幕数据库。

## 录制他人语音

在 VRChat 等多人环境中转写他人语音前，请确认符合 VRChat 社区规则和当地法律，必要时征得对方同意。
