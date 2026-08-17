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
- **字幕翻译与模型查询**：启用翻译、测试 API 配置或查询模型列表时，会连接用户选择的 DeepL、Microsoft Translator、OpenAI、Gemini、Alibaba Cloud 或 OpenAI 兼容服务。
- **学习分析**：只有用户在学习工作台中明确触发分析时，才会连接所选的 OpenAI、Gemini、Alibaba Cloud 或 OpenAI 兼容文本生成服务。请求包含当前学习项目的工作副本、所选词语、保存的译文、来源语言和有限词典快照；不会自动附带整段字幕历史、VRCX 房间信息或未选择的会话。
- **WebView2 运行时**：安装器在目标机器缺少 WebView2 时会联网安装（微软官方 bootstrapper）。
- **AnkiConnect**：制卡和连接检查只连接本机 AnkiConnect，默认地址为 `http://127.0.0.1:8765`。
- **OSC Chatbox**：用户启用该功能后，自己的麦克风识别文本、译文以及在 Chatbox 工作台中手动提交的内容会通过本机 UDP 发送给 VRChat，并可能作为聊天框内容显示给附近玩家。关闭 OSC 输出时不会发送。手动发送的内容会作为本机对话记录保存。
- **VRChat 静音同步**：启用后只通过本机 mDNS/OSCQuery 读取 VRChat 的 `MuteSelf` 布尔状态，用于暂停麦克风转写和阻止 Chatbox 输出；该状态不会写入字幕数据库或发送到外部服务。
- **第三方输出 API**：默认关闭。启用后会把用户订阅的识别原文、翻译结果和已发送 Chatbox 内容通过独立 WebSocket 输出。默认只监听本机回环地址；浏览器连接以及所有非回环监听都必须使用独立 Token 鉴权。订阅者收到的数据不会额外写入数据库。
- **VRCX-0 集成**：默认关闭。启用后只连接本机回环地址上的 VRCX-0 Integration API，读取当前世界名、成员显示名、是否为自己及成员语言。VRCS 不使用 VRCX-0 提供的用户 ID、房间 location 或成员备注作为模型上下文。

除上述情况外，查词和历史记录均不产生网络请求。

## 本地数据

- 字幕文本历史保存在本机 SQLite 数据库（桌面端：`%LOCALAPPDATA%\.vrcs\data\vrcs.db`；独立运行 Core 时相对于其配置文件保存）。
- 用户主动保存的学习项目、来源文本快照、结构化 AI 分析、卡片草稿和 Anki 笔记 ID 保存在同一 SQLite 数据库。清空字幕历史不会删除这些独立学习快照；删除学习项目也不会删除来源字幕。
- 学习工作台选择的 AI Profile、模型、解释语言和学习水平保存在桌面端本地偏好中。
- 设置与桌面偏好保存在本机配置文件与应用数据目录中。
- OpenAI Compatible 的自定义 HTTP Header 会以明文写入配置文件。VRCS 会拒绝 Authorization、Cookie、API Key 等敏感 Header；该功能只应用于 HTTP-Referer、客户端标题等非敏感元数据。
- 第三方输出 API Token 保存在系统凭据管理器的 `VRCS/ExternalAPI/token`，也可由 `VRCS_EXTERNAL_API_TOKEN` 环境变量提供，不写入配置文件或字幕数据库。
- VRCX-0 Token 保存在系统凭据管理器的 `VRCS/VRCXIntegration/token`，也可由 `VRCS_VRCX_INTEGRATION_TOKEN` 环境变量提供，不写入配置文件或字幕数据库。当前房间快照只保存在内存中，不创建历史记录。
- 卸载应用不会自动删除 `%LOCALAPPDATA%\.vrcs` 中的数据，需要时可手动删除。

## 诊断日志

- 桌面端将启动状态、组件版本、运行错误和技术堆栈按天保存在 `%LOCALAPPDATA%\.vrcs\logs\errorlog.YYYY-MM-DD.log`，默认最多保留最近 7 份。
- 日志只保存在本机，不会自动上传。用户可以从“设置 → Debug”打开日志目录或导出错误报告，并可在分享前自行检查内容。
- 错误日志不会主动记录原始音频、字幕正文、Chatbox 内容、翻译 Prompt、API Key、Token、Authorization Header 或完整请求体。技术错误可能包含本机文件路径；前端异常写入时会清理疑似敏感文本，导出报告时还会替换用户目录前缀并截断过长内容。
- 删除日志文件不会影响设置、模型或字幕历史；应用下次运行时会重新创建日志。

## Anki

制卡功能只连接本机的 AnkiConnect（默认 `http://127.0.0.1:8765`），发送的内容限于词条、读音、释义、语境和语言等笔记字段，不包含音频。

## LLM 翻译上下文

启用字幕翻译后，当前字幕文本会发送到用户选择的 DeepL、Microsoft Translator、OpenAI、Gemini、Alibaba Cloud 或 OpenAI 兼容 API。关闭翻译时不会发送；API Key 保存在系统凭据管理器或由环境变量提供，不写入配置文件或字幕数据库。

LLM 翻译的“附带最近原文”功能默认关闭。开启后，VRCS 会按用户选择读取最近的扬声器 final 字幕、麦克风 final 字幕和 Chatbox 原文，并与当前文本一起发送给所选 LLM Provider；DeepL 和 Microsoft Translator 不接收这些内容。本地 Profile 只会把请求发送到用户配置的本机地址。用户可分别关闭三个来源，设置 1–50 条消息和 200–12000 字符的上限，或关闭总开关停止附带历史原文。上下文在请求时从现有 SQLite 历史即时构造，不创建新的上下文存储。

启用 VRCX-0 集成及其 LLM 上下文开关后，当前世界名、成员显示名、是否为自己及成员语言会与字幕一起发送给所选 LLM Provider。该开关独立于“附带最近原文”；关闭最近原文不会阻止 VRCX-0 上下文。启用 VRCX-0 的 ASR 上下文开关后，当前世界名和成员显示名会在下一次开始转写时发送给 Qwen3 ASR 或 Fun-ASR；OpenAI Realtime 与本地 Whisper 不接收这些字段。关闭相应 VRCX-0 开关后不会发送。

## LLM 学习分析

学习分析默认按需触发，不会在字幕完成或查词后自动请求。云端请求只发送当前学习项目中可见且已保存的内容；Core 不自动加入 VRCX 世界名、成员列表或其他翻译上下文。选择标记为本地的 OpenAI 兼容 Profile 时，请求只发送到用户配置的本机地址，但本地服务是否继续转发由其自身设置决定。

模型返回内容会经过严格结构校验和长度限制后显示并保存。AI 解释可能包含错误或不确定推断，不能替代权威词典、教材或教师判断。诊断日志不记录完整学习文本、词典快照、Prompt 或模型完整响应。

## 录制他人语音

在 VRChat 等多人环境中转写他人语音前，请确认符合 VRChat 社区规则和当地法律，必要时征得对方同意。
