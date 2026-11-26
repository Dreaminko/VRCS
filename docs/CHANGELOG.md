<!--
  VRCS Release Changelog
  Use headings in the format: ## [x.y.z] - YYYY-MM-DD
  The release pipeline extracts the matching ## [x.y.z] section for latest.json and GitHub Release notes.
-->

# Changelog

## [0.4.2] - 2026-11-26

#### Features & Improvements

- **OSC Inbound Relay & shared OSCQuery discovery**: Added **OSC Inbound Relay** under **Settings → Connections** (listening on UDP port `9002` by default) to intercept `/chatbox/input` messages from third-party OSC apps, translate and route them through VRCS (via OSC, UniText, or both), and optionally forward unhandled OSC packets to VRChat with live packet counters and port-conflict checks. Unified **OSCQuery** discovery and advertisement across VRChat mute sync and UniText.
- **UniText manual Chatbox output & status**: Added an **Output target** selector (**OSC**, **UniText**, **Both**) in the Chatbox composer synced with Chatbox preferences. Sending to **UniText** bypasses VRChat's 144-character OSC limit in live previews and delivery, and surfaces UniText delivery status in the live view status bar.
- **Self VRChat Chatbox capture**: Added capture of your own in-game VRChat Chatbox messages from network traffic with echo suppression against recent VRCS-sent messages, preventing duplicate subtitles or translations.
- **Standalone network capture controls & OS-specific disclaimers**: Decoupled VRChat network capture into an independent standby and lifecycle control available from the desktop bottom dock, compact subtitle window, and VR controls/dashboard. Added OS-specific network capture disclaimers and help dialog details, including Linux experimental capture notices.
- **Compact subtitle modes, always-on-top & smooth reveal**: Added **Fixed-player** and **Scrolling** display modes, a **Keep window on top** toggle, native window resizing, and animated Chatbox reveal/collapse in the compact subtitle window.
- **Native webview interface scaling**: Switched desktop interface scaling to native webview zoom (`getCurrentWebview().setZoom`), removing CSS scale layout offsets in popovers, dropdowns, and resize anchors.

#### Bug Fixes & Improvements

- **Core capture, database & runtime performance**: Added read-only SQLite mode for query endpoints, incremental database vacuuming, non-blocking async glossary cache updates, event-driven (`FrameArrived`) Windows desktop capture, and optimizations across audio capture, VAD, player audio, VRChat log directory resolution/scanning, and local Qwen ASR / PLaMo runtime startup retries.
- **VR overlay & capture stability**: Modernized D3D11 texture readback and multithread protection, isolated VR surface resets, fixed VR dashboard settings routing, and hardened surface paint gating.
- **Core API, OSC & WebSocket hardening**: Hardened external API `/capabilities` auth, Linux keyring credential deletion rollback, settings validation and rollback for default model assignments, glossary refresh task lifecycle, OSC inbound/OSCQuery discovery, UniText fallback, and WebSocket backpressure handling.
- **Desktop UI, autosave & window reliability**: Hardened settings autosave concurrency and rollback, background model/runtime polling, SSE stream resync, Chatbox preferences sync, updater retry/error states, compact window transitions and tooltips, Windows save dialogs, elevation diagnostics, memory compression reporting, and the desktop region picker.

---

#### 新功能与体验优化

- **OSC 输入接管与共享 OSCQuery 发现**：在 **设置 → 连接** 中新增 **OSC 输入接管**（默认监听本地 UDP `9002` 端口），可接管第三方 OSC 应用发送的 `/chatbox/input` 消息，经 VRCS 翻译后通过 OSC、UniText 或混合模式输出，并支持将其他未拦截的 OSC 数据包透传至 VRChat，同时提供实时计数统计与端口冲突校验。统一 VRChat 静音同步与 UniText 的 **OSCQuery** 服务发现与广播逻辑。
- **UniText 手动 Chatbox 发送与状态提示**：Chatbox 发送栏新增与偏好同步的 **输出目标** 选择器（**OSC**、**UniText**、**混合**）。输出至 **UniText** 时，实时预览与发送不受 VRChat 144 字符 OSC 长度限制，并在实时界面状态栏显示 UniText 发送状态。
- **己方 VRChat Chatbox 捕获**：支持从网络流量中捕获自己发出的游戏内 VRChat Chatbox 消息，并对近期由 VRCS 发送的消息进行回声抑制，避免重复显示或重复翻译。
- **独立网络捕获开关与分平台说明**：将 VRChat 网络采集解耦为可独立待机与启停的控制项，在桌面底部导航栏、紧凑字幕窗口及 VR 控制／面板中均提供独立开关；同时新增分平台网络采集风险说明与帮助弹窗详情（含 Linux 实验性采集说明）。
- **紧凑字幕显示模式、置顶与展开动画**：紧凑字幕窗口新增 **固定玩家字幕** 与 **滚动字幕** 切换、**置顶窗口** 开关、原生窗口缩放命令以及平滑的 Chatbox 展开／收起动画。
- **原生 Webview 界面缩放**：将桌面端界面缩放迁移至原生 Webview 缩放，消除此前 CSS 缩放导致弹出层、下拉菜单与调整大小锚点的布局偏移问题。

#### 问题修复与体验改进

- **核心采集、数据库与运行时性能**：为查询接口引入只读 SQLite 模式与增量整理（incremental vacuum），将术语表缓存更新与刷新改为异步非阻塞，Windows 桌面捕获改为事件驱动（`FrameArrived`），并优化音频采集、VAD、玩家音频、VRChat 日志路径解析／扫描及本地 Qwen ASR / PLaMo 运行时启动重试逻辑。
- **VR 覆盖层与画面捕获稳定性**：升级 D3D11 纹理回读与多线程保护，隔离 VR 表面重置逻辑，修复 VR 仪表盘设置路由，并加固画面绘制门控。
- **核心服务、OSC 与 WebSocket 可靠性**：加固外部 API `/capabilities` 鉴权、Linux 密钥环凭据删除回滚、使用默认模型时的设置校验与回滚、术语表刷新任务生命周期、OSC 输入接管与 OSCQuery 发现、UniText 回退及 WebSocket 背压控制。
- **桌面端交互、自动保存与窗口行为**：修复并加固设置自动保存并发与回滚、后台模型／运行时轮询、核心事件流重同步、Chatbox 偏好同步、更新器错误与重试状态、紧凑窗口布局切换与提示气泡、Windows 保存对话框、提权启动诊断、内存压缩状态及桌面区域选择器。

**Full Changelog / 完整变更** · [0.4.1...0.4.2](https://github.com/Dreaminko/VRCS/compare/0.4.1...0.4.2)

## [0.4.1] - 2026-11-25

#### Features & Improvements

- **Follow translation settings in Chatbox**: The Chatbox composer can now follow your preferred translation language automatically via **Follow translation settings**, rather than requiring a manually pinned target language.
- **Configurable OSC message formats**: Moved **OSC message format** and **Custom message format** into **Settings → Connections → OSC**, sharing one format configuration across automatic voice translation and manual Chatbox sends. Custom templates now validate that both `{original}` and `{translation}` are included within 200 characters.
- **Redesigned Chatbox composer UI**: Refreshed the Chatbox composer layout, target-language picker, over-limit/truncation indicators, and expand/collapse transitions in both standard and compact modes.
- **In-app release notes & updater improvements**: Added a **View release notes** dialog that renders formatted Markdown release notes (with live GitHub fallback and automated appendix filtering) alongside a concise highlights preview in the update notification and settings card. Added download progress with transferred bytes, percentage, and speed, plus **Skip this version**, last-checked timestamp, and an unread update indicator on the bottom dock.
- **Structured release notes pipeline**: Stable releases now source notes directly from `docs/CHANGELOG.md`, and Nightly builds generate commit-based summaries for both update manifests and GitHub Releases.

#### Bug Fixes & Improvements

- **Chatbox translation consistency & language validation**: Discarded stale Chatbox translations when the target language or active room changes mid-request, and hardened language code validation in core configuration and OSC delivery.
- **Actionable updater errors & retry recovery**: Classified install failures into timeout, download, signature verification, and install errors with direct manual download links, and preserved pending update state so failed installs can be retried cleanly.

---

#### 新功能与体验优化

- **Chatbox 跟随翻译设置**：Chatbox 发送栏现可通过 **跟随翻译设置** 自动跟随首选翻译语言，不再强制固定目标语言。
- **可自定义 OSC 发送格式**：将 **OSC 发送格式** 与 **自定义发送格式** 移至 **设置 → 连接 → OSC**，让自动语音翻译与手动 Chatbox 发送共用同一套格式配置；自定义模板会校验同时包含 `{original}` 与 `{translation}` 且不超过 200 个字符。
- **Chatbox 发送栏界面重构**：优化标准模式与紧凑模式下的 Chatbox 布局、目标语言选择器、超长与截断提示以及展开／收起过渡动画。
- **应用内更新日志与更新器体验增强**：新增 **查看更新内容** 对话框，支持渲染 Markdown 格式的更新说明（支持实时拉取 GitHub 发布说明并过滤自动附加的平台说明），并在更新弹窗和设置卡片中展示要点预览。下载进度新增已下载大小、百分比与实时速度，同时支持 **跳过此版本**、显示上次检查时间，并在底部导航栏显示未读更新提示。
- **自动化发布说明流水线**：正式版发布流水线直接从 `docs/CHANGELOG.md` 提取对应版本的更新说明，Nightly 构建会自动按提交记录生成更新摘要并写入更新清单与 GitHub Release。

#### 问题修复与体验改进

- **Chatbox 翻译一致性与语言校验**：修复翻译期间切换目标语言或房间时可能写入过期译文的问题，并在核心配置与 OSC 发送中加强语言校验。
- **更新错误细分与重试恢复**：将更新安装失败细分为超时、下载失败、签名校验失败与安装失败等提示并提供手动下载入口；安装失败时保留待安装状态，支持直接重试。

**Full Changelog / 完整变更** · [0.4.0...0.4.1](https://github.com/Dreaminko/VRCS/compare/0.4.0...0.4.1)

## [0.4.0] - 2026-11-24

#### Features & Improvements

- **UniText**: Added UniText avatar prop OSC output integration, settings card, and repository link.
- **Player Identity**: Added VRChat player ID setting and self-speaker display across desktop and VR.
- **Chatbox Audio**: Added proximity filtering controls for Chatbox-only capture.
- **Onboarding**: Added theme selector to the onboarding welcome step.

#### Bug Fixes & Improvements

- **Linux VR**: Retained the last surface frame across short GPU stalls and retried snapshots missing frame markers.
- **VR Overlay**: Enforced surface paint serial ordering and improved error notice presentation.
- **Updater & Diagnostics**: Improved update checks, capture diagnostics, settings UI, and localization coverage.
