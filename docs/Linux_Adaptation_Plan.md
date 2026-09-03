# VRCS Linux 适配计划书

日期：2026-09-02  
状态：方案草案；尚未实施或完成 Linux 运行验证  
代码基线：`33184ee`，应用版本 `0.1.10`

## 1. 建议与目标

建议采用原生 Linux 适配，继续使用 React、Tauri 2 和嵌入式 Rust Core。先交付支持系统输出和麦克风的 CPU 桌面试用版，再补齐密钥保存与分发更新。VRChat 专用音频、CUDA 和头显 Overlay 各自作为后续扩展，避免其中一个受阻就无法交付桌面字幕。

首轮指定 Ubuntu 24.04 LTS、x86_64、glibc、PipeWire + pipewire-pulse 为支持基线，覆盖 GNOME 的 X11 与 Wayland 会话。Kubuntu 24.04 / KDE Plasma 作为第二轮桌面兼容性验证环境。这里是主动选择的范围，不代表已经验证，也不代表其他发行版无法运行。

最小交付物是一份可安装的 `.deb`：能选择音源、完成麦克风校准、生成并保存字幕，已有查词和学习界面可以使用。用户若只希望“先试试看”，完成里程碑 A 即可停止，后续阶段不会成为它的运行前提。

核心假设是用户有可用的 Linux 桌面音频服务，并接受第一版先提供桌面字幕。如果只有裸 ALSA，音频后端会明确报告服务不可用；如果必须首版就提供 SteamVR 头显字幕，则本计划的首版范围和工期需要扩大到第 7 节的 Overlay 工作。

## 2. 仓库现状与适配缺口

以下结论来自代码检查；“可复用”不等于 Linux 已通过测试。

| 模块 | 当前证据 | 适配动作 |
|---|---|---|
| 前端与桌面框架 | React 19、Tauri 2；前端 CI 已在 Ubuntu 运行，Rust CI 只在 Windows 运行 | 保留技术栈，增加 Linux 原生构建与 WebKitGTK 实机验收 |
| 音频边界 | `core/src/audio.rs` 已封装线程、停止与 channel；`audio/platform.rs` 在非 Windows 上直接报错 | 增加 Linux 后端，复用采集生命周期和识别管线 |
| VRChat 专用采集 | `server/capture.rs` 传入 `VRChat.exe`，Windows 后端通过进程 ID 回环采集 | 首版明确禁用；Linux 音频流选择单独设计 |
| API 凭据 | `credentials.rs` 使用 Windows Credential Manager；非 Windows 只支持环境变量 | A 保留环境变量入口；B 增加 Secret Service |
| 本地识别与 VAD | `whisper-rs 0.16`、`ort = 2.0.0-rc.12`；锁定的 `ort-sys` 分发表包含 Linux x86_64 CPU 包 | 保留 CPU 实现，验证原生依赖、模型装载和安装包运行 |
| CUDA | `asr/cuda.rs` 只在 Windows 加载 `nvcuda.dll`；Linux 即使启用 feature 仍走不支持分支 | 首版保持 CPU；后续单独实现 Linux 驱动探测 |
| VR Overlay | `vr_overlay/` 的文本绘制使用 GDI，纹理使用 D3D11；运行状态在非 Windows 为 Unsupported | 保留明确的不可用状态；后续替换渲染与运行时接入 |
| 桌面行为 | 已有 `xdg-open` 分支和非 Windows 置顶调用；托盘创建失败会使 setup 失败 | 处理托盘缺失、Wayland 置顶失败和紧凑窗口降级 |
| 数据与日志 | 数据使用 `local_data_dir()/.vrcs`；日志依赖 `LOCALAPPDATA`，否则落入临时目录 | 保留数据布局，修正 Linux 日志路径与路径脱敏 |
| 更新与发布 | `app_updates.rs` 固定 `windows-x86_64-*`；发布脚本和工作流生成 NSIS | A 禁用 Linux 自动更新；B 增加独立目标与 AppImage 签名 |
| OSC / Anki / VRCX-0 | 已有网络协议实现，VRCX-0 通过服务接口接入 | 复用实现，区分协议测试与外部程序实测 |

当前配置 schema 是 **26**，以 `core/src/config/schema.rs` 为准；`core/README.md` 中的旧 schema 说明不作为迁移依据。音频默认值保持 `output.mode=system`、`microphone.mode=disabled`、采样率 `16000`。

## 3. 技术方案

### 3.1 音频：通过 libpulse 接入现有桌面音频服务

采用 Linux 专用的 `libpulse-binding 2.30.1`，使用异步 API，由现有采集工作线程驱动事件循环。PipeWire 官方说明其 PulseAudio 协议服务使用原有 PulseAudio 客户端库，因此同一个后端可以面向 pipewire-pulse，并保留验证传统 PulseAudio 的可能。[PipeWire 协议说明](https://docs.pipewire.org/devel/page_module_protocol_pulse.html)、[Rust binding 文档](https://docs.rs/libpulse-binding/2.30.1/libpulse_binding/)

数据流保持现有结构：

```text
Linux source / sink monitor
           |
       audio/pulse
           |
 AudioCapture: f32 mono / 512 frames
           |
  VAD / Smart Turn -> ASR -> subtitle events
                                |       |
                             SQLite   HTTP / WebSocket -> React / Tauri
                                |
                     existing translation / learning / OSC paths
```

实施约束：

1. 麦克风枚举普通 source；系统输出枚举 sink，并采集其对应的 monitor source。将 monitor 与麦克风分组，避免用户选错音源。这里的“系统输出”指所选输出设备的混音，不承诺汇总所有声卡。
2. 申请既有 `audio.sample_rate` 对应的单声道 f32 流，由音频服务转换格式与采样率，再整理成每块 512 个采样交给下游。检查实际协商结果、静音数据块和不完整帧，不能假定每次回调长度相同。
3. 保留 `AudioDevice.id: i64`。Linux 用带后端和方向前缀的稳定 source/sink 名称生成 53 位范围内的 ID，不保存重启后可能变化的服务器 index。名称变化视为设备失效，要求重新选择；哈希冲突应报错，不能选中另一设备。
4. `device_id=null` 表示跟随系统默认设备；默认设备变化时重连对应流。指定设备消失则报告 `audio.device_unavailable`，不自动换到其他麦克风。系统输出也不能在 VRChat 专用采集失败时被静默启用。
5. libpulse context、stream 和回调对象由同一工作线程管理。事件循环必须定期处理停止信号和连接超时；不能让无数据时的阻塞读取挂住 `join()`。channel 满时不阻塞音频回调，记录溢出并结束该次采集，通过已有错误流程恢复。
6. 将公共调用中的 `wasapi_id` 改为中性名称，Windows 设备 ID 算法和 WASAPI 行为保持不变。使用条件编译选择 `wasapi`、`pulse` 或 unsupported 后端，无需引入插件框架或新服务。

不建议首版直接采用原生 PipeWire 图管理：它适合后续复杂路由，但当前只需要设备输入和输出 monitor，libpulse 已提供这些接口。也不采用长期启动 `parec` / `ffmpeg` 子进程的产品实现，避免新增进程生命周期、外部工具安装和错误转译边界。相关工具只用于验收。

### 3.2 能力声明与界面行为

在现有 `/health` 响应增加只读 `audio_capabilities`，字段为 `backend`、`microphone`、`system_loopback`、`process_loopback`。它描述后端实现能力，不代替 `/api/audio/devices` 对当前服务和设备可用性的检查；健康检查不执行阻塞式设备枚举。

Linux 首版的 `backend=pulse`、前两项采集能力为 true、`process_loopback=false`。设置页和首次向导据此禁用 VRChat 专用来源并说明原因。保存与启动接口也校验能力，不能仅靠前端隐藏。Windows 继续使用原有支持条件和运行时错误判断。

从 Windows 复制来的 `mode=vrchat` 或设备 ID 可以读取，但必须引导重新选择；不自动保存替代音源。保留 schema 26、数据库结构、既有音源枚举值和 HTTP/WebSocket 鉴权边界。新能力字段为增量字段，旧客户端可忽略。

Linux Alpha 的云服务密钥仍使用现有环境变量，界面明确显示此限制并禁用尚不能生效的保存操作。系统默认音源、音频错误和凭据说明改为平台适用文案，四种现有语言同步更新。

### 3.3 凭据：第二阶段接入 Secret Service

Linux 专用依赖采用 `keyring 3.6.3`，显式启用 `sync-secret-service` 与 `crypto-rust`。保留 Windows 当前实现。`service=VRCS`，条目标识使用现有完整 target 字符串，例如 `VRCS/API/profile/{profile_id}`，覆盖 API profile、External API 和 VRCX-0 三类令牌。[keyring 后端与 feature 文档](https://docs.rs/keyring/3.6.3/keyring/)

环境变量继续优先。凭据状态增补 `storage_state`，取值为 `available`、`locked`、`unavailable` 或 `environment_only`；`source` 增加 `secret_service`。锁定或不可访问时，界面显示“无法读取已保存凭据”，不能把它显示为“未保存”。有效环境变量不能因为凭据库查询失败而失效。

凭据调用离开 UI 和 Tokio 核心执行线程，并串行访问存储。应用启动及普通状态刷新不主动弹出解锁请求；用户明确保存或使用已保存凭据时处理解锁、取消及失败。不将密钥降级写入配置文件，不自动导出或迁移 Windows Credential Manager 的内容。无 Secret Service 的环境仍可用 CPU 与环境变量模式。

### 3.4 桌面、路径与模型

- **紧凑窗口**：切换尺寸和布局不依赖置顶成功。X11 验证置顶；Wayland 无法置顶时保留紧凑窗口并说明限制，停止反复重试同一不支持的操作。上游存在 Wayland 置顶限制，不能承诺全部窗口管理器一致。[Tauri 上游问题](https://github.com/tauri-apps/tauri/issues/3117)
- **托盘**：创建失败不阻止程序启动。Linux 首版停用“关闭到托盘”，关闭窗口正常退出；保留任务栏最小化。可用的托盘通过菜单提供“显示”和“退出”，不依赖左键事件。Tauri 明确说明 Linux 不发送这些鼠标事件。[Tauri System Tray](https://v2.tauri.app/learn/system-tray/)
- **开机启动**：默认仍为关闭。B 验证现有 autostart 插件在登录桌面会话时启动；AppImage 路径变动后重新注册，不添加系统级常驻服务。
- **数据目录**：沿用 Tauri `local_data_dir()/.vrcs`，Linux 通常为 `$XDG_DATA_HOME/.vrcs`，未设置时通常为 `~/.local/share/.vrcs`。日志统一写入该目录下的 `logs/`，新增 `HOME` / XDG 路径脱敏。不要为了目录美观迁移既有 Windows 数据。独立 Core 的 `VRCS_CONFIG` 等入口保持原语义。
- **模型与推理库**：延续现有模型下载、哈希校验、`.part` 文件与离线复用。CPU 构建检查 ONNX Runtime / whisper.cpp 的链接产物及运行时依赖；可执行文件能启动但模型无法推理，不算适配成功。Silero / Smart Turn 缺失继续遵循已有降级规则。
- **CPU 可移植性**：发布构建设置 `GGML_NATIVE=OFF`，避免按构建机生成本机专用指令；用第二台 CPU 实测，依据实际构建依赖记录最低 CPU 能力，不宣称覆盖所有 x86_64 处理器。

## 4. 两个可独立交付的里程碑

工期按一名熟悉仓库的工程师、可用 Linux 测试机估算；包含实现和对应验证，不包含等待硬件、凭据或发布审核。

| 里程碑 | 工作与交付物 | 验收边界 | 估算 |
|---|---|---|---|
| A：CPU 桌面 Alpha | Linux 音频后端、能力驱动音源选择、平台文案、日志与窗口降级、Linux Rust CI、`.deb` 试用包 | 系统输出 + 麦克风 + CPU 字幕链路可用；云服务仅用环境变量；VRChat 专用来源、CUDA、Overlay、自动更新不可用且说明清楚 | 7–10 人日 |
| B：日常使用 Beta | Secret Service、AppImage 打包与签名、平台更新目标、桌面集成验收、OSC 和已配置外部服务联调、安装说明 | Ubuntu 基线及 Kubuntu 复核通过；密钥跨重启保存；AppImage 可安全更新；`.deb` 手动更新 | 5–8 人日 |

合计 **12–18 人日**，加上环境差异余量，日历上建议预留 **3–5 周**。这是工程估算，不是已经完成试编译后的承诺。

A 的内部顺序是：条件编译与 Linux 构建 → 音频枚举和采集 → 能力校验与界面 → 安装包和端到端验收。各小项属于同一功能交付，不把只能编译、不能转写的状态称为完成。

B 可在 A 已经可用的基础上独立合并；即使 B 延后，A 仍然可以安装、转写和保存历史。

## 5. 文件范围与依赖

预计涉及 **超过 8 个文件**，但不增加后端服务、不更换语言或运行时。下面是实施落点，不是本次已经修改的文件。

| 范围 | 主要文件 | 责任 |
|---|---|---|
| 音频后端 | `core/src/audio.rs`、`core/src/audio/platform.rs`、`core/src/audio/wasapi/`；新增 `core/src/audio/pulse/` | 保持公共行为，新增枚举、采集、设备事件与生命周期处理 |
| API 与验证 | `core/src/server.rs`、`core/src/server/capture.rs`、`core/src/server/settings/change_plan/validation.rs` | 能力字段、音源校验、稳定错误码 |
| 凭据 | `core/src/credentials.rs`、`core/src/server/cloud.rs`、`core/src/server/external.rs`、`core/src/server/vrcx.rs` | Secret Service、状态与非阻塞调用边界 |
| 前端 | `src/capture/`、`src/core-client/`、`src/onboarding/`、`src/settings/`、`src/shared/protocol/credentials.ts`、`src/shell/useCompactWindow.ts`、`src/i18n/locales/` | 音源与凭据状态、向导、平台文案；路径前缀为 `apps/desktop/` |
| 原生桌面 | `apps/desktop/src-tauri/src/lib.rs`、`diagnostics.rs`、`app_updates.rs` | 目录、托盘、置顶降级和更新平台选择 |
| 构建 | 两份 `Cargo.toml` 和相应 lockfile、根 `package.json`；新增 `tauri.linux.conf.json`、Linux 构建脚本 | Linux 专用依赖和产物；保留现有 Windows 构建入口 |
| CI / 分发 | `.github/workflows/ci.yml`、`release.yml`、`scripts/build-release.ps1` 相关元数据生成逻辑 | 新增 Linux job，统一汇总更新清单 |
| 验证与说明 | 相关 Rust 测试、`apps/desktop/tests/`、四种语言 README、`THIRD_PARTY_NOTICES.md` | 记录实测范围、平台差异、依赖和安装方法 |

拟新增的直接 Rust 依赖只有 A 的 `libpulse-binding` 和 B 的 `keyring`，均限定 Linux target，并在实施时锁定传递依赖。本计划不安装依赖；后续实施批准需涵盖这两个明确列出的新增依赖。

Ubuntu 构建环境需要 Tauri 官方列出的 WebKitGTK 4.1、GTK/AppIndicator 等开发包，另加项目原生构建所需的 `cmake`、`pkg-config`、`clang`、`libclang-dev`、`libpulse-dev`，B 增加 `libdbus-1-dev`。Node 使用仓库要求的 24+，Rust 使用 stable。Linux 桌面需要可用的音频服务；B 的持久凭据需要实现 Secret Service 的桌面密钥库。[Tauri Linux prerequisites](https://v2.tauri.app/start/prerequisites/)

开发和离线 CPU 验收不需要新账号。云服务实测使用用户已有服务商密钥；Anki 制卡需要用户运行的 AnkiConnect；VRCX-0 实测需要其服务与已有令牌。发布更新使用已有 `TAURI_SIGNING_PRIVATE_KEY`、可选密码及 `TAURI_UPDATER_PUBLIC_KEY`，GitHub 发布权限沿用当前项目。缺少其中某项时，只暂停对应外部集成或发布验证，不阻止本地 CPU 验收。

## 6. 分发、验收与回滚

### 6.1 包与更新

A 新增独立 `tauri.linux.conf.json`，生成 `VRCS-<version>-linux-x64.deb` 和 SHA-256；配置不继承 NSIS 安装钩子。Linux 的 `updater_available=false`，原生更新命令也拒绝执行，不能只隐藏按钮。版本号沿用项目发布规则，不在适配阶段预先递增正式版本。

B 增加 `VRCS-<version>-linux-x64.AppImage`、签名及 SHA-256。AppImage 使用 `linux-x86_64-standard` 更新目标，保留两个既有 `windows-x86_64-*` 目标；`.deb` 始终通过安装新包或包管理器更新，关闭应用内安装更新。Tauri 的 Linux updater 产物是 AppImage，因此两种包必须区分运行方式。[Tauri Updater](https://v2.tauri.app/plugin/updater/)

CI 分别构建 Windows / Linux，再由一个汇总步骤合并各平台更新条目，检查版本、URL、签名、散列和目标集合后生成唯一 `latest.json`。某个平台失败时，不覆盖生产更新清单。预发布试用包不写入正式 `latest` 更新入口；正式发布另按仓库发布权限执行。

Linux 包固定在所支持的最老构建基线 Ubuntu 24.04 生成，不随 `ubuntu-latest` 自动提升 ABI 下限。AppImage 仍受 glibc 等宿主条件限制，不作为“所有 Linux 发行版通用”的承诺。[Tauri AppImage 兼容性说明](https://v2.tauri.app/distribute/appimage/)

### 6.2 自动化验证

实施后在 Linux 基线执行以下命令；这些是未来验收命令，本次计划编写未执行它们：

```bash
npm ci
npm run check:i18n
npm --workspace apps/desktop test
npm run build:frontend
cargo fmt --manifest-path core/Cargo.toml -- --check
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path core/Cargo.toml --all-targets --locked -- -D warnings
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets --locked -- -D warnings
cargo test --manifest-path core/Cargo.toml --locked
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --locked
```

新增 `npm run build:linux` 作为 `.deb` CPU 构建入口；B 新增 `npm run build:linux:appimage`，封装上述 Linux 配置、`GGML_NATIVE=OFF` 和签名要求。命令失败必须返回非零状态，不产生可被误当作成功发布的清单。

新增测试只覆盖适配风险：Linux ID 稳定性和方向分离；数据分块及静音；启动超时、停止和背压；默认设备变化与指定设备丢失；不支持音源的保存/启动拒绝；凭据锁定、取消与环境变量优先；更新平台与包格式选择；紧凑模式在置顶失败时仍可用。依赖真实音频或密钥库的测试显式分组，无会话的 CI 不伪装成硬件测试。

保留现有 Windows Rust CI 和前端测试，验证 Windows 编译及公共行为。发布前分别运行 Windows 标准/CUDA 打包；在没有对应 GPU 时，只记录构建通过，不宣称 CUDA 运行通过。

### 6.3 实机验收

| 场景 | 通过条件 |
|---|---|
| 干净安装 | 普通用户安装并启动，WebKitGTK 界面正常，无未找到的动态库；数据目录可写 |
| 系统输出与麦克风 | 各自采集、双路采集、校准、停止、重新开始都可用；来源标签正确；连续 30 分钟无崩溃、无持续增长的音频积压 |
| 设备与服务异常 | 拔出设备、默认设备切换、音频服务重启、无设备均有明确反馈；服务恢复后可重新开始；正常停止在 2 秒内完成 |
| 本地识别 | 模型下载与校验成功；准备好模型后断网仍可转写；用第二台 CPU 运行安装包；记录模型、硬件与端到端延迟 |
| VAD / Smart Turn | 正常模型可装载；模型缺失、损坏或不可下载时走既有降级，状态显示真实后端 |
| 数据与学习 | 字幕历史跨重启保留；词典导入、查词、学习素材保存正常；自定义模型目录与含中文/空格路径可用 |
| X11 / Wayland | 拖动、缩放、输入法、中日文字体、紧凑模式、文件选择正常；没有托盘时仍能显示和退出；置顶限制不阻止字幕 |
| 密钥与云服务（B） | 保存/读取/删除和重登录持久化通过；锁定、取消、无服务均可解释；日志与配置不泄露密钥；至少一个已配置云 ASR/翻译服务实测 |
| VRChat OSC（B） | 用本机实际运行的 VRChat 验证 Chatbox 与 OSCQuery；`MuteSelf=true` 或未知时仍阻止自动发送；仅 UDP 测试通过不能替代此项 |
| 可选外部集成（B） | 有对应环境时验证 AnkiConnect 制卡和 VRCX-0 上下文；缺少环境则标记未验，不宣传已验证 |
| 更新（B） | AppImage 从旧试用版本升级，数据保留；错误签名、断网、只读位置不会损坏当前版本；`.deb` 不调用 AppImage updater；Linux 不下载 Windows 包 |

性能采用同一模型、同一音频样本和相近硬件与 Windows 比较。首轮记录延迟、CPU、内存和失败情况；本计划不编造尚未测得的性能指标。正式支持声明只包含已实际通过的发行版、桌面会话和集成组合。

### 6.4 回滚与停止条件

A / B 不新增数据库迁移，功能失败可卸载试用包或回退代码，保留数据与模型目录。Linux 设备选择和密钥不会自动转换为 Windows 设备/密钥；跨系统移动数据时重新选择音源、填写凭据，并先备份配置与数据库。

发布包出现平台性故障时暂停该 Linux 更新条目，保留可安装的上一版本；已安装较新版本的回退通过手动安装完成，不假定 updater 支持自动降级。不得通过关闭签名校验或删除用户数据来恢复。

出现以下证据就收缩支持范围：目标环境没有可用音频服务时停在“不可采集”并提供修复信息；Wayland 不支持置顶时交付普通紧凑窗口；没有实机音频测试时交付构建产物但不宣称音频验收完成；没有头显与 Linux SteamVR 环境时不进入 Overlay 扩展。

## 7. 后续扩展的边界

以下扩展不计入 A / B 的完成条件，也不需要一次全部开展。由维护者根据桌面 Beta 的使用反馈决定优先级。

| 扩展 | 推荐方向 | 必须额外证明 | 粗估 |
|---|---|---|---|
| VRChat 专用音频 | 枚举应用播放流，用 libpulse 的 sink-input monitor 捕获明确识别的目标；不要把 Windows PID 回环直接换成 Linux PID 查找 | Proton 下应用属性与宿主进程的对应、同应用多流、流重建/换设备、游戏重启；无法唯一识别时明确失败，禁止偷偷采集全部系统声音 | 4–7 人日 |
| Linux CUDA | 给 `asr/cuda.rs` 增加 Linux 驱动库探测并复用现有 whisper CUDA feature；独立 Linux CUDA 包和更新目标 | 实际设备枚举、模型推理、驱动不匹配、无 GPU、显式 CUDA 拒绝与 auto 模式回退；CPU 包不依赖 CUDA | 3–5 人日 |
| SteamVR Overlay | 保留现有字幕呈现状态机；Linux 接入 OpenVR，替换 GDI 文本绘制与 D3D11 纹理，先头显字幕，再手腕交互 | 文本整形与字体回退、透明度、纹理提交、头显/手柄坐标、输入事件、运行时断开恢复和真实头显性能 | 10–20 人日，环境风险最高 |

PulseAudio 已公开针对 sink-input 的监视接口，但这不足以证明任意 Proton 组合都能可靠识别 VRChat。相关运行时行为必须由扩展阶段的实机测试确认。[PulseAudio PCM 监视说明](https://wiki.freedesktop.org/www/Software/PulseAudio/Documentation/Developer/Clients/WritingVolumeControlUIs/)

OpenVR 有 Linux SDK 支持，但项目当前的 Windows 绘制、纹理与进程检测需要分别处理。Overlay 扩展的渲染库和纹理路径不在桌面方案中提前锁定；该选择由负责 Overlay 的工程师在获得目标头显与 SteamVR 环境后提出独立方案。这里是明确延后的产品范围，不是 A / B 的实现缺项。[Valve Linux OpenVR 说明](https://github.com/ValveSoftware/SteamVR-for-Linux)

首轮还不包含 ARM64、musl/Alpine、Flatpak/Snap 沙箱发布、纯 ALSA 后端、AMD/Intel GPU 加速或 VRChat/Proton 本身的兼容性修复。

## 8. 本计划的证据边界

已完成仓库代码、构建脚本、平台条件编译和官方接口文档检查；已核对锁定的 ONNX Runtime Linux CPU 分发记录与 whisper-rs-sys 的构建参数入口。未执行 Linux 编译、录音、云服务请求、安装包运行、CUDA 或 SteamVR 实测。

实施从里程碑 A 开始。完成 A 的安装包和实机验收后，先交付用户试用，再决定是否继续 B 及后续扩展。
