# VRCS 本地 QwenASR 支持计划书

日期：2026-09-02  
状态：设计提案，尚未实施  
范围：Windows x64 桌面应用及独立 Rust Core

## 1. 建议与目标

建议增加 **Qwen3-ASR 0.6B INT8 本地识别**，通过 sherpa-onnx 的 Rust 接口嵌入现有 Core。首版采用 CPU 推理，复用现有音频采集、VAD 和断句逻辑，提供“说完一段后显示字幕”的体验。Whisper 和云端服务继续保留，现有用户不会被自动切换到新引擎。

这条路线可以提供应用内下载模型、离线识别和统一设置体验，无需用户另装 Python、Docker 或 WSL。它值得作为一个可选引擎加入，但是否适合实时 VRChat 使用，要以双音源和游戏同时运行时的测量结果决定。不能把官方服务器吞吐量或原始模型成绩直接用作桌面 INT8 版本的性能承诺。

首版完成的用户路径是：进入识别设置 → 选择“本地 Qwen3-ASR” → 下载并校验模型 → 开始采集 → 获得带来源标识的最终字幕 → 继续使用已有翻译、历史、Overlay 和 Chatbox 功能。

## 2. 项目现状与接入依据

本计划以当前代码为依据；配置 schema 为 **26**。部分 README 中的旧 schema 描述不能作为迁移依据。

| 已有能力 | 接入价值与必须处理的限制 |
|---|---|
| Rust Core 直接嵌入 Tauri，也可独立运行 | 可以在同一进程中增加本地识别引擎，无需新增常驻服务 |
| `core/src/asr/engine.rs` 的 `AsrEngine` | 已接收音频样本并返回文本与可选语言，适合包装 Qwen 离线识别 |
| `AsrService` 及模型预加载、设置回滚 | 可继续负责引擎生命周期，但当前构造流程只支持 Whisper |
| `core/src/pipeline.rs` 的本地分段路径 | 可以复用 VAD/Smart Turn；目前按 `local_whisper` 字符串决定是否走本地路径 |
| `core/src/pipeline/dependencies.rs` | 已统一处理最终文本、数据库与下游发布，两个音源共用一个加锁的本地识别服务 |
| Whisper 模型下载和迁移 | 有进度、取消、SHA-256 校验和跨盘迁移，但清单假设每个模型只有一个文件 |
| `ort = 2.0.0-rc.12` | VAD 和 Smart Turn 已依赖 ONNX Runtime，新引擎必须处理运行库共存 |
| 分段上传与 OpenAI 音频转写适配 | 可以为外部本机服务方案复用一部分逻辑，但不等于已支持任意 Qwen 服务协议 |

实际配置与源码默认值均采用 0.4 秒静音断句、6 秒最大语音段。源码默认识别服务与用户保存的识别选择是不同概念；新增功能不改写其中任何一个已有选择。

## 3. 已核实的上游能力

| 事实 | 对方案的影响 |
|---|---|
| Qwen 官方发布 0.6B、1.7B 两种 ASR 模型，覆盖中、英、日等语言，权重采用 Apache-2.0 许可 | 首版选择较小模型；分发时记录权重、转换产物及推理组件的许可和来源 |
| 官方 `qwen-asr` 提供 Transformers 和 vLLM 后端；官方文档当前将流式推理限定在 vLLM 后端 | 官方具备流式能力，不代表所选 Windows 本地后端具备同样能力 |
| vLLM 官方仍不提供原生 Windows 支持 | 不把 vLLM 作为普通 Windows 安装包的必需组件 |
| sherpa-onnx 提供 `sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25` 模型及 Rust 离线识别示例 | 可以用已有 Rust/C++ 技术栈接入；这是第三方转换和推理实现，不是 Qwen 官方 Windows SDK |
| 模型包含前端、编码器、解码器及 tokenizer，整套文件约 1 GB | 下载和存储管理要支持整套文件；磁盘体积不等于推理内存 |
| 核对版本为 sherpa-onnx `v1.13.7`，有 Windows x64 CPU 发布包；其 Windows CPU 构建使用 ONNX Runtime `1.27.1` | 固定版本并控制 DLL 来源，不能任由两个 Cargo 构建脚本各自覆盖同名运行库 |

依据：[Qwen 官方仓库](https://github.com/QwenLM/Qwen3-ASR)、[官方模型卡](https://huggingface.co/Qwen/Qwen3-ASR-0.6B)、[vLLM 安装要求](https://docs.vllm.ai/en/latest/getting_started/installation/gpu/)、[sherpa-onnx 模型说明](https://k2-fsa.github.io/sherpa/onnx/qwen3-asr/pretrained.html)、[固定版本 Rust 示例](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.7/rust-api-examples/examples/qwen3_asr.rs)、[Windows 发布包](https://github.com/k2-fsa/sherpa-onnx/releases/tag/v1.13.7)。

## 4. 首版范围

首版交付以下能力：

- 独立的“本地 Qwen3-ASR”选项，0.6B INT8、CPU 执行，支持系统音频和麦克风。
- 自动语言识别，以及适配器明确支持的固定识别语言；首先验收中文、日语、英语。
- 应用内模型下载、进度、取消、完整性校验、删除及存储目录迁移。
- 最终字幕沿用现有下游协议、存储和分发路径。
- 清晰展示模型未安装、加载中、运行失败和推理积压状态。
- 标准版与现有 CUDA 版均提供 Qwen CPU 模式；CUDA 版中的 Whisper GPU 能力不代表 Qwen GPU 能力。

以下内容不纳入本次实现：真正的流式增量识别、逐词时间戳、ForcedAligner、说话人分离、1.7B 模型、Qwen GPU/DirectML/Vulkan 加速、任意用户模型导入、微调、Qwen 专用术语或 VRCX 上下文增强。它们不构成首版可用的前置条件。

CPU 路线的价值是部署一致和避免占用游戏显存；CPU 竞争仍可能影响 VRChat，必须实测。

## 5. 技术方案

### 5.1 引擎与数据流

```text
WASAPI：系统音频 / 麦克风
             |
       16 kHz 单声道 PCM
             |
       Silero VAD / Smart Turn
             |
       有界本地识别调度
             |
        AsrEngine 接口
       /              \
 WhisperEngine    QwenAsrEngine
                       |
             sherpa-onnx / ONNX Runtime
       \              /
        现有最终字幕发布路径
             |
   SQLite / WebSocket / 翻译 / Overlay / Chatbox
```

新增 `QwenAsrEngine`，通过 sherpa-onnx `OfflineRecognizer` 识别分段样本。每段创建独立 stream，模型实例常驻并在双音源之间串行复用，避免两套权重常驻内存。识别结束返回现有 `Transcription`，不扩展字幕数据库 schema。

该方案不隔离原生库崩溃：上游部分初始化错误会直接终止进程，Rust 的错误返回不能捕获这种行为。因此首版只加载经过完整校验的固定模型，并验证初始化失败路径；若合法模型和受支持环境仍可复现进程终止，阻止发布，另行评估进程隔离，不能声称所有底层错误都可在设置页恢复。

集中提供“是否本地后端”和“当前应加载哪个本地引擎”的判断，替换启动、验证、能力查询、设置切换及管线中的 Whisper 专属分支。只做本次需要的两引擎分派，不引入通用插件注册框架。

语言代码要映射为 Qwen 接口使用的语言名称，例如 `zh` → `Chinese`、`ja` → `Japanese`、`en` → `English`；自动模式不设置 stream 的 `language` 选项。语言选项取应用和模型支持集合的交集，不静默接受无效映射。该版本 Rust 结果结构没有可靠的自动识别语言字段，因此自动模式返回 `language = None`，固定语言模式返回已设置的代码，继续使用现有下游语言处理逻辑，不伪造检测结果。

首版把 Qwen 单段上限限制为 6 秒；实际生效值为用户配置上限与 6 秒中的较小值，并在设置中说明。更长的持续讲话由现有分段器切段，不能先接收长段再截掉尾部。Smart Turn 也必须遵守此硬上限。Qwen 解码配置先固定 `max_total_len = 512`、`max_new_tokens = 256`，由密集语速样本验证容量；触及容量上限应报告失败，不能把截断结果当作完整字幕发布。

上游接口依据：[Qwen 识别实现](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.7/sherpa-onnx/csrc/offline-recognizer-qwen3-asr-impl.cc)、[Rust 离线接口](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.7/sherpa-onnx/rust/sherpa-onnx/src/offline_asr.rs)。

### 5.2 配置、界面与失败策略

| 接口 | 首版决定 |
|---|---|
| `asr.backend` | 新增 `local_qwen3_asr`，保留 `local_whisper` 和全部现有云端值 |
| `asr.local` | 继续保存原有 Whisper 设置，切换到 Qwen 不覆盖它 |
| `asr.qwen.model` | 新增，默认 `qwen3-asr-0.6b-int8`，首版仅接受这一 ID |
| `asr.qwen.num_threads` | 新增，默认 2，接受 1–8；实际使用不超过可用逻辑 CPU 数量 |
| Qwen 执行设备与精度 | 首版固定 CPU、INT8，在界面说明，不增加无实际选择的配置开关 |
| `asr.language` | 复用现有选择与语言预设，按后端能力验证 |
| `cloud_failure_policy = local` | 继续回退到原有 Whisper，首版不增加可选回退引擎 |
| 本地 Qwen 失败 | 报告错误并允许用户切回其他引擎；不自动把音频上传云端 |
| 配置迁移 | schema 26 → 27，仅补充 Qwen 默认配置，保留现有识别选择和数据 |

设置页、首次使用向导、顶部引擎名称和启动前检查都要识别新后端。模型卡明确标注“CPU / INT8 / 断句后识别”。切换到本地后端不删除 API 配置或凭据；不需要为本地 Qwen 建立 API profile。

首次选择但尚未下载模型时，可以保存该选择，启动采集前要求完成下载。采集中更换引擎时，复用现有候选预加载及事务回滚：候选加载失败就保留原配置和原引擎，不显示虚假的切换成功。预加载可能短暂同时持有两套模型，内存压力下允许用户停止采集后再切换。

### 5.3 调度与资源边界

目前本地转写会在管线内等待阻塞推理完成，不能把模型换上以后就假设双音源不会积压。增加一个共用的本地推理 worker，继续使用单个 `AsrService`；推理在阻塞线程执行，音频采集和 VAD 循环继续运行。

- 等待队列最多 2 段，另允许 1 段执行中；每个音源最多占用一个等待槽，队列按入队顺序处理。
- 入队失败时跳过当前段并发送现有失败事件，错误码为 `asr.queue_full`，不能无限增加等待任务或静默丢弃。
- 排队超过 3 秒的段不再开始识别，报告 `asr.queue_expired`；此阈值是首版行为约束，后续只凭测量调整。
- 每个任务保留音源、utterance ID 和采集代次。停止采集、重启或切换引擎后，旧代次结果不能进入字幕、翻译或 Chatbox。
- `spawn_blocking` 的取消不能中断正在运行的原生推理。停止时取消排队段并屏蔽旧结果，正在执行的调用结束后再回收资源；不能把异步超时称为已终止计算。
- 采集停止后释放 Qwen 实例；切换时保持原有设置事务所需的短暂回滚窗口。

这一改动涉及 Whisper 共用路径，因此必须有 Whisper 回归验证，但不扩展到云端调度重构。

### 5.4 多文件模型管理

保留已有 Whisper 文件及 ID。把模型清单从单文件记录扩展为带引擎类型和文件列表的模型包；Whisper 作为单文件包使用同一套下载与校验机制。

Qwen 文件安装在现有 `storage.model_directory` 下的 `qwen3-asr-0.6b-int8/` 子目录中。旧配置即使使用 `models/whisper` 这个目录名也保持原路径，不额外引入第二个存储设置。`VRCS_ASR_MODEL_DIR` 继续覆盖整个 ASR 模型目录，已有迁移行为扩展为同时处理两类模型。

安装流程：

1. 固定下载 sherpa-onnx 指定的 2026-03-25 模型包。实现时将包的精确大小、SHA-256，以及下列文件的精确大小和 SHA-256 写入版本化清单；构建和运行时均不从可变远程清单接受新哈希。
2. 下载到临时位置，校验包后解压到独立暂存目录，仅接受 `conv_frontend.onnx`、`encoder.int8.onnx`、`decoder.int8.onnx` 和 tokenizer 的 `vocab.json`、`merges.txt`、`tokenizer_config.json`。许可材料另按分发清单保留。
3. 限制解压总量；拒绝绝对路径、路径穿越、链接及模型清单外的可执行文件，不安装上游测试音频。
4. 全部文件校验通过后原子安装；下载中断、取消或校验失败不会产生“已安装”状态，也不破坏已有模型。
5. 运行前验证完整文件集合，复用现有文件变化检测缓存；初始化失败时强制重验。删除和跨盘迁移覆盖整套文件，失败保留旧目录与配置。

下载前检查压缩包、解压文件、已有模型及暂存空间的实际总和，并留 10% 余量。运行时内存需求单独测量，不用下载大小推断。

继续使用 `/api/asr/models` 和现有下载、删除路由。列表增加 `engine`、显示名称及包级进度；已有 Whisper ID 不变。`/api/asr/capabilities` 增加按引擎划分的设备、语言和增量识别能力，旧的 Whisper/CUDA 字段保持兼容，前端以新字段渲染 Qwen。

### 5.5 ONNX Runtime 与安装包

采用 **sherpa-onnx 1.13.7 shared 链接 + 单份 ONNX Runtime 1.27.1 CPU DLL**。sherpa 的 Rust crate 默认是 static，接入时需关闭默认特性并启用 shared，避免默认静态链接带入另一套 ORT。

现有 `ort` 保持 rc.12，改用显式动态加载，保留项目实际使用的 `std`、`ndarray`、`tracing` 和 `api-24` 特性，关闭自动下载和自动拷贝 ORT 的默认行为。在创建任何 VAD、Smart Turn 或 Qwen session 前，从受控的应用运行库目录初始化 ORT。开发运行、独立 Core、测试二进制和安装包都必须使用同一份已校验运行库。

构建准备脚本固定 sherpa Windows x64 shared MD Release 包与哈希，通过 `SHERPA_ONNX_LIB_DIR` 提供库目录；不依赖构建时自动抓取浮动版本。打包时显式检查 sherpa 及 ORT DLL 已被安装到可解析的程序目录，避免只在开发机 PATH 中能找到。标准版和 CUDA 版共用这套 Qwen CPU 库；原 Whisper CUDA 构建保持原用途。

这是 ABI 兼容性设计，不是已经完成的兼容性验证。若 ORT API 检查、Silero、Smart Turn 或安装包启动回归失败，本版本不能交付该组合；不以同时塞入两份同名 DLL 的方式绕过问题，也不自动扩大为新增 Python 服务。

计划新增的直接 Rust 依赖为 `sherpa-onnx`，以及读取上游模型 `.tar.bz2` 所需的 `tar`、`bzip2`；具体依赖和原生资源校验记录随实现进入 lockfile/清单。本计划不安装依赖。Qwen 推理无需 API key 或第三方账户，首次下载需要网络；模型与既有 VAD/Smart Turn 资源准备完成后，ASR 可以离线工作。翻译或学习分析是否联网仍取决于用户原来的设置。

链接依据：[sherpa Cargo 特性](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.7/sherpa-onnx/rust/sherpa-onnx/Cargo.toml)、[原生构建脚本](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.7/sherpa-onnx/rust/sherpa-onnx-sys/build.rs)、[Windows ORT 版本](https://github.com/k2-fsa/sherpa-onnx/blob/v1.13.7/cmake/onnxruntime-win-x64.cmake)。

## 6. 实施工作包与工期

按一个开发者熟悉现有代码估计，首版需要 **12–18 个工作日**。这是排期估算，不是性能或完成日期承诺。本次会超过 8 个文件，主要原因是配置、模型管理、推理调度、前端和打包存在真实耦合；不新增进程或服务。

这些是同一个首版的内部工作包，不把尚不能独立使用的中间状态称为已交付功能。完整验收后作为一个可用版本发布。

| 工作包 | 交付物 | 估算 |
|---|---|---:|
| 引擎与原生运行库 | 固定依赖、可复现 DLL 准备流程、Qwen 引擎、语言映射；VAD/Smart Turn 共存 | 3–4 天 |
| 模型与配置 | 多文件清单、事务安装、迁移/删除、schema 迁移、能力接口 | 3–4 天 |
| 管线与用户流程 | 有界调度、停止/切换防旧结果、设置与向导、四种界面语言 | 3–5 天 |
| 验收与交付 | 音频质量/延迟测量、Whisper 回归、双版本安装包、使用与许可说明 | 3–5 天 |

主要文件边界：

| 区域 | 文件或目录 |
|---|---|
| 依赖与初始化 | `core/Cargo.toml`、`core/src/lib.rs`、`core/src/startup/`、两处 Cargo.lock |
| 本地引擎 | `core/src/asr.rs`、`core/src/asr/engine.rs`、新增 `core/src/asr/qwen.rs` |
| 本地调度 | `core/src/pipeline.rs`、`core/src/pipeline/dependencies.rs`、新增 `core/src/asr/local_dispatcher.rs` |
| 模型包 | `core/src/asr/model.rs`、`manager.rs`、`download.rs`、`migration.rs` |
| 配置与切换 | `core/src/config/recognition.rs`、`schema.rs`、`migration.rs`、`validation.rs`；`core/src/server/capture.rs`、`models.rs`、`settings/change_plan/` |
| 设置与类型 | `apps/desktop/src/recognition-services.ts`、`providers/types.ts`、`settings/recognition/`、模型管理组件、`transcription-start.ts` |
| 向导与语言 | `apps/desktop/src/onboarding/`、`i18n/locales/`，对应现有测试 |
| 打包与文档 | `scripts/build-release.ps1`、新增 `scripts/prepare-asr-runtime.ps1`、`apps/desktop/src-tauri/tauri.release.conf.json`、相关 CI、README、`THIRD_PARTY_NOTICES.md` |

实现时只修改这些区域中确实参与功能的文件，保留原有未提交变更。

## 7. 验证与验收标准

### 7.1 必须通过的行为检查

| 场景 | 通过条件 |
|---|---|
| 配置迁移 | schema 26 迁移后原识别选择、Whisper 设置、API 配置、音频设置保持一致 |
| 模型安装 | 取消、中断、损坏、缺文件、空间不足和非法解压路径均不会留下可加载的半成品 |
| 目录管理 | 中文/空格路径、跨盘移动、删除下载中模型；失败可恢复且原配置可用 |
| 正常识别 | 中/英/日的固定语言和自动模式工作，最终字幕音源正确，零字幕重复入库 |
| 边界音频 | 静音、极短音频、混合语言、噪声、连续说话、快速语速；没有未提示的截断、越界或崩溃 |
| 调度 | 双音源同时输入、队列满、过期、反复停止/启动；旧结果不能进入新会话及 Chatbox |
| 热切换 | Qwen ↔ Whisper ↔ 云端；加载失败保留旧配置，原云端到 Whisper 的回退策略仍有效 |
| 运行库 | 同一安装环境内 Silero、Smart Turn、Qwen 都能创建 session，实际加载路径和版本符合清单 |
| 离线与隐私 | 预下载后断网仍可识别；本地 ASR 不发起云端音频请求，不持久化原始音频 |
| 分发 | 在没有 Python、CUDA 和开发机 PATH 的 Windows 环境安装标准版并识别；CUDA 版同时验证 Whisper CUDA 和 Qwen CPU |

### 7.2 质量和性能门槛

以下均为**拟定验收目标，尚未测得**。测试记录必须附 CPU、内存、Windows 版本、线程数、模型哈希、VRChat 场景及是否双音源。验收参考机采用至少 6 核/12 线程 CPU、16 GB RAM 的 Windows 11 x64 机器；这不是对最低配置的承诺。

- 使用同一份人工核对转写的固定语料对比 Whisper small 与 Qwen：中、英、日各至少 10 分钟，另含至少 10 分钟混合语言、背景声和 VRChat 常见语音；原始录音只作为经授权的测试素材，不作为产品录音功能。
- 中文/日语记录 CER，英语记录 WER；Qwen 在至少一种主要目标语言上有可复现收益，其余语言错误率的绝对退化不超过 2 个百分点。未满足时可以保留实验候选，但不宣称准确率更好。
- RTF 定义为推理耗时除以音频时长。双音源持续运行时，两路实际推理需求之和应低于可用时间，并留至少 20% 余量；不能以单路 RTF 小于 1 就判定双路可实时运行。
- 在 VRChat 运行时，1–5 秒短句的语音结束至最终字幕延迟 P95 目标不超过 2.5 秒，包括断句、排队和推理，不包括翻译。
- 连续双音源运行 30 分钟，目标为无队列溢出、无持续积压、无崩溃；稳态内存目标相对仅加载 VAD 的同等会话增量不超过 3 GiB，并单独记录引擎切换峰值。
- 同一可重复 VRChat 场景中，CPU frame time P95 的增加目标不超过 10%；同时记录 VR 重投影变化，避免用平均帧率掩盖卡顿。

最脆弱的假设是：0.6B INT8 在目标 CPU 上能与 VRChat 及双音源同时保持上述延迟。如果不成立，首版只能以明确标注限制的实验选项交付，或暂缓公开入口；不自动转用云端，不用增大队列掩盖吞吐不足。GPU 加速需另立范围并确定运行库和模型精度组合。

### 7.3 实现后的验证命令

先运行改动对应的配置、模型包、调度和前端测试，再执行首版集成检查：

```powershell
.\scripts\test-core.ps1
npm --workspace apps/desktop test
npm run check:i18n
npm run build:frontend
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
```

使用已准备好的固定原生运行库执行这些命令。真实模型音频检查必须显式提供已校验的模型和测试素材；未提供时报告“未执行”，不能让测试静默跳过后仍作为验收依据。安装包另按现有标准版/CUDA 版构建流程验证；现有发布签名凭据仅在正式打包时使用，计划和开发测试不需要索取新凭据。

本计划阶段只完成代码、配置、上游接口和发布信息核对，没有下载模型、安装依赖、运行推理、构建安装包或执行性能测试。

## 8. 更小方案与后续边界

若只需要让已有自建环境的用户试用，最小方案是新增 **连接本机 Qwen 服务**的适配器，估计 2–4 天客户端工作量。复用现有分段和音频编码，通过 loopback 连接用户自行启动的服务；客户端不负责 Python、模型、WSL 或服务生命周期。

该方案不能仅把现有阿里云 Qwen WebSocket 改成 localhost。Qwen 官方服务示例使用带音频内容的 `/v1/chat/completions`，也不能直接假设它支持当前 `/audio/transcriptions` 适配器；需要独立请求适配、输出标记清理、可选本机鉴权及连接测试。该方式减少客户端分发工作，但把安装和维护成本转给用户，因此不作为普通桌面用户的主方案。

首版完成后，再根据测量选择是否开展 Qwen GPU、1.7B 或增量字幕。三项分别需要新增验收，不作为本次承诺：GPU 要证明游戏负载下的总收益，1.7B 要证明质量收益足以抵偿资源增加，增量字幕要处理文本修订和最终结果一致性。

## 9. 发布与回滚

新后端保持可选，更新使用说明和四种界面文案，明确区分“本地 Qwen3-ASR”与“阿里云 Qwen 实时识别”。记录转换模型、原生库和依赖的许可信息；安装包内不包含大型 ASR 权重。

用户层面的回退是切回已保留的 Whisper 或云端设置。新版本内回退无需删除模型、修改字幕历史或迁移数据库。

程序降级需要同时回退配置与原生运行库，不能假设旧版能读取 schema 27。迁移前按现有机制备份 schema 26 配置；降级时使用备份，并明确提示迁移后新增设置可能需要重新配置。Qwen 模型目录可以留存，不主动删除。正式发布、上传安装包、建立 release 或提交 PR 属于后续交付动作，本计划不会执行这些操作。
