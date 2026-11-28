<!--
  VRCS Release Changelog
  Use headings in the format: ## [x.y.z] - YYYY-MM-DD
  The release pipeline extracts the matching ## [x.y.z] section for latest.json and GitHub Release notes.
-->

# Changelog

## [0.4.4] - 2026-11-28

#### Features & Improvements

- **Native GPU UI renderer for VR overlays**: Replaced WebView2/WGC and Linux WebKitGTK VR surface capture with a direct GPU UI renderer (`vr_overlay::gpu_ui`, powered by `taffy`, `cosmic-text`, `resvg`, `pulldown-cmark`, and D3D11/Vulkan painters). Removed legacy VR surface webview hosts, recovery loops, and smoke/webview surface code, and updated `vr-preview` to render directly via the native GPU UI.
- **Interactive VR layout edit mode & 6DoF wrist menu placement**: Added an in-VR **Layout Edit Mode** launched from the VR Dashboard with a floating edit toolbar (**Auto Level**, **Face HMD**, overlay visibility toggles, **Reset Item**, **Reset All**, **Save & Exit**, **Cancel**), direct controller grabbing/pushing, and corner-handle uniform scaling for Headset, Wrist, and Wrist Menu overlays. Added full 6DoF pose and width configuration (`offset_x_m`, `offset_y_m`, `offset_z_m`, `pitch_deg`, `yaw_deg`, `roll_deg`, `width_m`) for the wrist menu with automatic legacy field cleanup on config load.
- **Independent speaker & microphone preferred ASR languages**: Expanded ASR recognition language selection into ordered multi-language lists (up to 4 preferred languages, schema v48 → v49), separated speaker and microphone language settings and presets across Core, desktop UI, and the VR Dashboard, and added smooth list/chip transitions.
- **UniText player ID toggle & VR Dashboard controls**: Added a `show_player_id` option for UniText output in player audio mode across Core, desktop settings, and the VR Dashboard, along with unified OSC/UniText dashboard controls and a wrist quick-menu toggle on the VR Display settings page.

#### Bug Fixes & Improvements

- **GPU UI CJK font consistency & locale syncing**: Stabilized CJK font family selection, weight snapping, and italic fallback in the GPU UI text engine, and synchronized dashboard locale updates to active VR surfaces.
- **Shared wrist placement & microphone single-lane captions**: Unified OCR and Vision wrist overlays to share the wrist conversation overlay's position, bound hand, and width (keeping independent opacity and font size). Forced single-speaker ASR and collapsed microphone captions into a single lane without speaker metadata.
- **Localization & frontend cleanup**: Relocated `errors.conversations` i18n keys out of updater errors, pruned unused translation keys, and removed leftover CSS zoom rules and obsolete VR control/smoke test artifacts.

---

#### 新功能与体验优化

- **VR 覆盖层原生 GPU UI 渲染器**：使用直接 GPU 渲染器（`vr_overlay::gpu_ui`，基于 `taffy`、`cosmic-text`、`resvg`、`pulldown-cmark` 及 D3D11/Vulkan 绘制管线）取代 Windows WebView2/WGC 与 Linux WebKitGTK 表面捕获；移除旧版 VR Webview 宿主、恢复逻辑与兼容表面代码，并让 `vr-preview` 直接使用原生 GPU UI 渲染。
- **VR 交互式布局编辑模式与手腕菜单 6DoF 定位**：在 VR 仪表盘新增 **编辑布局** 入口与悬浮编辑工具栏（支持 **自动水平**、**面向视野**、各面板显隐开关、**重置当前**、**全部重置**、**保存并退出** 与 **取消**），支持在 VR 中直接抓取、推拉及拖拽角标等比缩放头显字幕、手腕对话与手腕菜单面板；为手腕菜单新增完整 6DoF 位置/旋转/宽度配置，并在配置加载时自动清理已废弃的旧字段。
- **独立的扬声器与麦克风多选 ASR 识别语言**：将 ASR 首选识别语言扩展为最多 4 项的有序语言列表（配置架构升级至 v48 → v49），在核心服务、桌面设置与 VR 仪表盘中将扬声器与麦克风的识别语言及语言预设彻底拆分为独立配置，并优化列表与语言标签的过渡动画。
- **UniText 玩家 ID 开关与 VR 仪表盘控制**：在玩家音频模式下为 UniText 输出新增 **显示玩家 ID**（`show_player_id`）开关，覆盖核心配置、桌面设置与 VR 仪表盘，并在 VR 仪表盘中整合 OSC/UniText 设置与手腕快捷菜单开关。

#### 问题修复与体验改进

- **GPU UI 中日韩字体一致性与语言同步**：修复并稳定 GPU UI 文本引擎的中日韩字体族选择、字重对齐与斜体回退逻辑，并在界面语言变更时同步刷新活跃 VR 表面。
- **共享手腕面板位置与麦克风单栏字幕**：统一 OCR 与视觉问答手腕面板复用手腕对话面板的绑定手、位置、角度与宽度（仅保留各自字号与不透明度）；为麦克风强制单人识别并剥离发言人元数据，将麦克风字幕合并为单栏展示。
- **本地化与前端清理**：将 `errors.conversations` 翻译键从更新器错误分组迁出并清理未使用的翻译键，同时移除残留的 CSS `zoom` 规则与已废弃的 VR 烟雾测试及协议类型。

**Full Changelog / 完整变更** · [0.4.3...0.4.4](https://github.com/Dreaminko/VRCS/compare/0.4.3...0.4.4)

## [0.4.3] - 2026-11-26

#### Features & Improvements

- **Smooth conversation switching & history caching**: Added directional exit/enter transition animations (respecting reduced-motion preferences) and delayed pulsing loading skeletons when switching conversations in the Live view and Learning Materials browser. Added an in-memory LRU cache (up to 8 conversations) with background revalidation and live-subtitle buffering during switches to eliminate blank flashes when revisiting recent conversations.
- **Concise VRChat / UniText send status labels**: Shortened top status bar labels for VRChat Chatbox and UniText sending states (including **Discovering** and **Not detected**) across all supported locales (`en-US`, `zh-CN`, `zh-Hant`, `ja-JP`) for cleaner status presentation.

#### Bug Fixes & Improvements

- **WebView2 memory management & idle VR surface reclamation**: Added Windows WebView2 low-memory mode when the main window is minimized to the system tray (restoring on show) and WebView2 suspend/resume for hidden OCR/Vision windows and sleeping VR overlay surfaces. Automatically tears down idle VR overlay WebView2 environments after 30 seconds of inactivity (re-initializing on demand) and shortens cold-eviction timeouts to 15 seconds for transient VR surfaces (OCR and Player Audio).
- **Core-side source language detection**: Moved fallback source-language detection from the desktop frontend (`tinyld`, removed from the frontend bundle) to the Rust core (`whatlang` with CJK/Kana/Hangul heuristics), attaching `detected_language` to WebSocket live partials/finals, conversation subtitle history APIs, and VR overlay captions to eliminate async detector loading races and extra re-renders.
- **Desktop memory & rendering optimizations**: Unmounted the full desktop shell while Compact mode is active, removed eager idle preloading of Learning and Settings chunks, loaded only the active UI locale bundle (unloading the previous bundle on language switch), tightened in-memory subtitle history limits (400 items / 1M characters), cleared inactive Learning inbox state, reset stale material selection on conversation switch, scoped speaker identity subscriptions per speaker, and limited live translation-partial subscriptions to actively translating or the 20 most recent messages.

---

#### 新功能与体验优化

- **平滑对话切换与历史缓存**：在实时界面与学习素材浏览中切换对话时，新增带方向感的退出／进入过渡动画（遵循系统减少动态效果偏好）与延迟显示的骨架屏占位。同时新增最多缓存 8 个对话的内存 LRU 历史缓存，支持后台静默刷新与切换期间实时字幕暂存，消除来回切换近期对话时的白屏闪烁。
- **精简 VRChat / UniText 发送状态文案**：缩短顶部状态栏中 VRChat 聊天框与 UniText 的发送状态提示（含 **发现中**、**未检测到** 等状态），覆盖全部界面语言（`en-US`、`zh-CN`、`zh-Hant`、`ja-JP`），使状态栏展示更紧凑清晰。

#### 问题修复与体验改进

- **WebView2 内存管理与空闲 VR 表面回收**：在 Windows 端为主窗口最小化至托盘时启用 WebView2 低内存模式（恢复显示时还原），并为隐藏的 OCR／视觉选择窗口及休眠的 VR 覆盖层表面启用 WebView2 挂起／恢复；当所有 VR 覆盖层表面空闲超过 30 秒时自动回收 WebView2 环境（需要时按需重建），并将 OCR 与玩家音频等临时 VR 表面的冷回收超时缩短至 15 秒。
- **核心侧源语言检测**：将备用源语言检测从桌面前端迁移至 Rust 核心服务（移除前端 `tinyld` 依赖，改为核心侧 `whatlang` 结合中日韩字符启发式规则），直接在 WebSocket 实时事件、对话字幕历史接口及 VR 覆盖层字幕中附带 `detected_language`，减小前端包体并消除异步加载检测器带来的竞态与二次刷新。
- **桌面端内存与渲染优化**：进入紧凑模式时卸载主桌面壳层而非仅隐藏，移除学习与设置页面的空闲预加载；启动与切换界面语言时仅保留当前生效的语言包并卸载旧语言包；收紧内存字幕历史上限（400 条／100 万字符），离开学习页面时清理收件箱缓存，切换对话时重置过期素材选中状态，将发言人身份订阅细化到单个发言人，并将实时译文预览订阅限制在正在翻译或最近 20 条消息范围内。

**Full Changelog / 完整变更** · [0.4.2...0.4.3](https://github.com/Dreaminko/VRCS/compare/0.4.2...0.4.3)

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
