<!--
  VRCS Release Changelog
  Use headings in the format: ## [x.y.z] - YYYY-MM-DD
  The release pipeline extracts the matching ## [x.y.z] section for latest.json and GitHub Release notes.
-->

# Changelog

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
