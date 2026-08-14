# LLM Provider、翻译上下文与第三方输出 API 实现方案

本文基于 VRCS 当前 `main` 分支的实现状态，规划以下能力：Gemini 原生 API、增强的 OpenAI Compatible、Provider 能力注册表、Provider 无关的翻译上下文与可编辑 Prompt，以及正式的第三方输出 API。

本文是实现交接文档，不表示相关功能已经完成。各阶段必须可独立合并；后续阶段未实施时，已发布阶段仍应保持完整可用。

## 1. 目标与结论

推荐按以下发布优先级实施：

1. Provider 能力注册表与 Gemini 原生 API。
2. OpenAI Compatible 品牌预设、可选鉴权、自定义 Header、超时和诊断。
3. Provider 无关的翻译上下文、术语表与可编辑 Prompt。
4. 独立、版本化的第三方事件 WebSocket API。

Provider 能力注册表是第一阶段的内部基础，并与 Gemini 一起交付，不单独发布纯重构版本。

首期 Gemini 使用原生 `generateContent`、`streamGenerateContent` 和 `models.list` REST 接口，不使用 Google 的 OpenAI 兼容入口。Google 在 2026 年已将 Interactions API 作为新项目的首选，但 VRCS 当前的 `LlmRequest` 是无服务端会话的单轮文本任务，`generateContent` 与现有抽象的映射更直接，改动面也更小。Gemini 协议必须封装在独立适配器内，以便将来在不改翻译业务层的前提下切换到 Interactions API。

## 2. 范围

### 2.1 本方案包含

- Gemini 独立 Provider、API Key、模型列表、非流式和 SSE 流式输出、稳定错误码。
- OpenAI Compatible 继续使用一个协议实现，通过预设表达 DeepSeek、Groq、OpenRouter、LM Studio 和 Ollama 的默认连接参数。
- 本地兼容服务允许 `auth_mode = none`，不再强制保存 API Key。
- 模型列表自动加载失败时保留现有手动输入能力。
- 非敏感自定义 HTTP Header、每个 Profile 的请求超时、分阶段连接诊断。
- 后端作为 Provider 元数据和能力的唯一来源，前端不再按品牌重复推断。
- 最近原文上下文、来源开关、术语表、Prompt 编辑、长度限制和隐私提示。
- 独立的第三方事件服务、版本化事件信封、订阅、鉴权和生命周期事件。
- 配置迁移、单元测试、前端测试、隐私与用户文档更新。

### 2.2 本方案不包含

- Gemini ASR、Gemini Live API、图片、音频、工具调用或服务端会话。
- 为 Groq、OpenRouter、DeepSeek、LM Studio 或 Ollama 复制独立 Provider 实现。
- 任意自定义鉴权 Header。首期自定义 Header 只允许非敏感元数据；API Key 仍由 Windows 凭据管理器或环境变量提供。
- 自动识别本地服务品牌或扫描本机端口。
- 自动从 VRChat 获取玩家名、世界名或游戏术语。首期术语由用户维护。
- Prompt 的 YAML 文件导入、导出或热重载。VRCS 继续使用现有 JSON 配置和设置界面，不新增 YAML 依赖。
- 第三方客户端向 VRCS 写入字幕、控制采集或发送 Chatbox 消息。首期 API 只输出事件。
- 修改现有内部 `/ws` 协议。桌面端继续使用该端点，第三方 API 使用独立监听器。

## 3. 当前实现盘点

| 领域 | 当前状态 | 缺口 |
|---|---|---|
| API Profile | `ApiProfile` 已支持命名配置、供应商、用途、Base URL 和凭据状态 | 能力判断分散在 Rust 与 TypeScript；没有 Gemini、预设、Header、可选鉴权和 Profile 超时 |
| 通用 LLM | `core/src/llm/mod.rs` 已支持 OpenAI Responses、OpenAI Chat Completions、Alibaba 兼容接口、模型列表和 SSE 增量文本 | 仍以品牌字符串分派；DeepSeek 通过 URL/模型名匹配；不支持 Gemini 原生协议 |
| 连接测试 | `/api/asr/profiles/{id}/test` 已能执行一次真实翻译；`/{id}/models` 已能获取模型列表 | 结果只有成功或统一错误；模型查询要求凭据；没有端点级诊断和耗时信息 |
| 模型选择 | 设置页会自动请求模型列表，`EditableDropdownField` 已允许失败后手动输入 | 无需重新实现手动回退；只需为无 Key、本地服务和诊断补齐后端行为与提示 |
| 翻译 Prompt | LLM 翻译使用编译期常量 `TRANSLATION_INSTRUCTIONS` | 用户不可编辑；没有上下文和术语表 |
| 对话原文 | `subtitles` 已保存 `speaker`、`microphone`、`chatbox`；`chatbox_messages` 单独保存手动消息原文 | Chatbox 在 `translation` 发送模式下的 conversation subtitle 可能是译文，不能只读取 `subtitles` 作为原文上下文 |
| 翻译事件 | 已有 started、partial、completed、failed 广播 | 事件只服务内部 UI；没有版本信封、事件 ID、稳定消息 ID 和订阅 |
| ASR 事件 | 已有 partial、failed、audio level；最终文本通过 `subtitle` 事件发布 | partial 与 final 的关联标识没有贯穿到最终字幕；事件模型不适合作为稳定第三方契约 |
| 网络安全 | Core 默认监听 `127.0.0.1`，内部 HTTP 使用 Bearer token，内部 WebSocket 使用 query token；非回环监听要求 token | 第三方消费者不应依赖桌面端每次启动生成的内部 session token，也不应因此获得全部内部 REST API 权限 |

现有 `SubtitleLifecyclePublisher` 已将字幕生命周期与具体输出协议解耦，是新增第三方事件出口的主要复用点。现有 SQLite 原文、Chatbox 原文记录和广播通道也足以支持上下文与第三方输出，无需重新设计主数据流。

## 4. 目标架构

```text
                         +----------------------+
                         | Provider Registry    |
                         | metadata/capability  |
                         +----------+-----------+
                                    |
           +------------------------+------------------------+
           |                                                 |
 +---------v----------+                           +----------v---------+
 | API Profile/UI     |                           | LLM Client         |
 | presets/diagnostic |                           | protocol adapters  |
 +--------------------+                           +----------+---------+
                                                               |
                               +-------------------------------+----------------+
                               |                                                |
                    +----------v-----------+                         +----------v---------+
                    | Translation Service |                         | Gemini/OpenAI/etc.  |
                    | prompt/context task |                         | remote/local API    |
                    +----------+-----------+                         +--------------------+
                               |
              +----------------+----------------+
              |                                 |
 +------------v-------------+       +-----------v-------------+
 | Internal /ws + Desktop   |       | External Event Server   |
 | existing contract        |       | versioned read-only API |
 +--------------------------+       +-------------------------+
```

架构约束：

- 翻译业务层只构造任务，不判断 Gemini、Groq 或 Ollama。
- Provider Registry 管理展示与能力；协议适配器管理请求和响应差异。不要把 DeepSeek 的请求参数等协议细节暴露为 UI 能力字段。
- 品牌预设只是 `openai_compatible` Profile 的默认值，不是新的 Provider ID。
- 内部 `/ws` 与第三方事件 API 分离，前者可以继续迭代桌面 UI，后者必须遵守版本兼容策略。

## 5. 阶段一：Provider 能力注册表与 Gemini 原生 API

### 5.1 Provider 类型

加入原生 Provider ID：

```text
gemini
```

现有 `ApiProfile` 字段已足够表达 Gemini：`provider = gemini`、`purpose = llm`，Base URL、区域和 Workspace 均为空。阶段一不改变配置 JSON 结构，`SCHEMA_VERSION` 保持 v11，避免为了新增枚举值制造无意义迁移。

### 5.2 Provider Registry

新增 `core/src/providers.rs`，提供静态 Provider 定义和按 Profile 解析后的能力：

```rust
pub struct ProviderCapabilities {
    pub supports_streaming: bool,
    pub supports_model_listing: bool,
    pub requires_api_key: bool,
    pub is_local: bool,
    pub supports_context: bool,
    pub supports_translation: bool,
    pub supports_asr: bool,
    pub supported_languages: LanguageCoverage,
}

pub enum SupportLevel {
    Native,
    ProtocolCompatible,
}

pub struct CapabilitySupportLevels {
    pub asr: Option<SupportLevel>,
    pub translation: Option<SupportLevel>,
}
```

支持级别按能力记录，不能只给 Provider 一个总标签。Alibaba Cloud 的 ASR 是原生协议，LLM 翻译使用其 OpenAI 兼容入口；OpenAI 和 Gemini 的翻译是原生协议；OpenAI Compatible 的翻译是协议兼容。注册表至少包含 Alibaba Cloud、OpenAI、Gemini、OpenAI Compatible、DeepL 和 Microsoft Translator。阶段一把现有 OpenAI Compatible Profile 解析为“需要 API Key、远程服务”；阶段二加入预设后，`requires_api_key`、`is_local` 和模型列表能力改由 Profile 的预设、`auth_mode` 与 `is_local` 字段解析，不写死在品牌定义中。

新增只读接口：

```http
GET /api/providers
```

响应包含 Provider ID、显示名、各用途的支持级别、能力、允许的用途和 `presets` 数组。阶段一的 `presets` 为空，阶段二再填入兼容预设。`GET /api/asr/profiles` 的每个 Profile 同时返回解析后的能力，供列表页直接展示。前端删除 `api-profile-purpose.ts` 中按品牌维护的能力逻辑，改为消费后端数据；TypeScript 只保留类型与无副作用的展示辅助函数。

UI 按 Profile 用途显示 `原生支持` 或 `协议兼容`。Alibaba Cloud 的 shared Profile 显示“ASR 原生支持，LLM 协议兼容”，避免把兼容端点误标成原生实现。

### 5.3 Gemini 原生适配器

在 `core/src/llm/` 拆分协议模块：

```text
mod.rs                 公共 LlmClient、LlmRequest、LlmError 与分派
openai.rs              OpenAI Responses
openai_compatible.rs   Chat Completions 与 SSE
gemini.rs              Gemini 原生 REST 与 SSE
alibaba.rs             Alibaba 兼容入口及其固定区域 URL
```

Gemini 默认 Base URL 固定为：

```text
https://generativelanguage.googleapis.com/v1beta
```

接口映射：

| 能力 | Gemini 原生接口 |
|---|---|
| 非流式生成 | `POST /models/{model}:generateContent` |
| 流式生成 | `POST /models/{model}:streamGenerateContent?alt=sse` |
| 模型列表 | `GET /models` |
| 鉴权 | `x-goog-api-key` Header |

`LlmRequest.instructions` 映射到 `systemInstruction.parts[].text`，`input` 映射到单个 user `contents`。流式适配器累计 `candidates[].content.parts[].text`，每次回调传递累计文本，与现有 OpenAI Compatible 的 `LlmProgress` 语义一致。模型列表只保留 `supportedGenerationMethods` 包含 `generateContent` 的模型，并从名称中去掉 `models/` 前缀。

Gemini 错误转换为现有稳定错误命名空间：

| HTTP/错误条件 | 稳定错误码 |
|---|---|
| 400 | `llm.invalid_request` |
| 401/403 且为凭据问题 | `llm.authentication_failed` |
| 404 模型不存在 | `llm.model_not_found` |
| 429 | `llm.rate_limited` |
| 请求超时 | `llm.timeout` |
| 5xx | `llm.provider_unavailable` |
| 成功响应无文本或 SSE 非法 | `llm.invalid_response` |

`detail` 保留 Google 返回的安全诊断文字，但 UI 只根据稳定错误码选择用户提示。日志不得记录 API Key、完整 Prompt 或上下文原文。

连接测试新增可选的 `model` 参数。前端优先传用户当前选择的模型；没有选择时，后端从 Gemini 模型列表中选择第一个支持 `generateContent` 的模型，不能沿用 OpenAI 的默认模型名。

### 5.4 阶段一验收

- 可以创建 Gemini Profile、保存和删除凭据、测试连接、列出模型并手动选择模型。
- 手动翻译与自动翻译均可使用 Gemini；自动翻译通过现有 WebSocket 产生 partial 和 completed 事件。
- Gemini 不经过 `/openai/` 兼容端点。
- Provider 列表和 Profile 列表展示“原生支持/协议兼容”及真实能力。
- 现有 v11 配置无需迁移，读取和保存后结构不变。
- OpenAI、Alibaba Cloud、DeepL、Microsoft Translator 和现有 OpenAI Compatible 回归通过。

## 6. 阶段二：完善 OpenAI Compatible

### 6.1 Profile 配置与迁移

将配置 schema 从 v11 升级到 v12，`ApiProfile` 新增：

```rust
pub preset_id: Option<String>
pub auth_mode: ApiAuthMode // bearer | none
pub is_local: bool
pub timeout_ms: u64
pub headers: Vec<HttpHeaderConfig>
```

v11 → v12 迁移规则：

- 所有现有 Profile 的 `preset_id` 为 `None`。
- 所有现有 Profile 的 `auth_mode` 为 `bearer`。
- 所有现有 Profile 的 `is_local` 为 `false`。
- `timeout_ms` 为 `8000`，保持当前 `TranslationService` 的 8 秒行为。
- `headers` 为空数组。
- Provider、Base URL、用途、模型与凭据目标名不变。

`HttpHeaderConfig` 的首期规则：

- Header 名符合 HTTP token 语法，名称和值分别限制为 128 和 2048 字节，最多 16 项。
- 禁止 `Authorization`、`Proxy-Authorization`、`Cookie`、`Set-Cookie`、`X-Api-Key` 和 `X-Goog-Api-Key`，避免把秘密写入 JSON 配置。
- 禁止覆盖 `Content-Type`、`Accept` 和 `User-Agent` 等由客户端管理的 Header。
- Header 值会出现在本地配置文件中，设置页必须明确显示“不要填写密钥或令牌”。

### 6.2 预设而非新 Provider

在 Provider Registry 中定义以下预设：

| `preset_id` | Base URL | 默认鉴权 | 本地 |
|---|---|---|---|
| `deepseek` | `https://api.deepseek.com/v1` | Bearer API Key | 否 |
| `groq` | `https://api.groq.com/openai/v1` | Bearer API Key | 否 |
| `openrouter` | `https://openrouter.ai/api/v1` | Bearer API Key | 否 |
| `lm_studio` | `http://127.0.0.1:1234/v1` | 无 | 是 |
| `ollama` | `http://127.0.0.1:11434/v1` | 无 | 是 |
| `custom` | 用户填写 | 用户选择 | 用户声明 |

选择预设时自动填充字段，但保存后仍把实际 `base_url` 写入 Profile，使配置在预设默认值将来变化时保持稳定。修改预设不自动覆盖用户已编辑的 URL、Header 或超时。

DeepSeek 的 `thinking`、`temperature` 和 `stream_options` 请求差异改由内部 `ProtocolBehavior` 处理，删除当前通过 URL 包含 `deepseek` 或模型名前缀进行品牌匹配的逻辑。其他预设共享标准 Chat Completions 请求结构。

### 6.3 可选鉴权与模型列表

- `auth_mode = bearer` 时沿用凭据管理器；缺少 Key 时连接测试和实际调用返回 `translation.credential_missing`。
- `auth_mode = none` 时不读取凭据，也不发送空的 `Authorization: Bearer` Header。
- 模型列表接口接受无 Key Profile。失败时返回结构化诊断，但设置页继续保留现有可编辑模型输入框。
- 自动模型加载失败不阻止保存 Profile，也不清除用户已填写的模型。

### 6.4 连接测试与兼容性诊断

保留现有测试 URL，但返回结构化结果：

```json
{
  "ok": true,
  "latency_ms": 243,
  "checks": [
    { "name": "endpoint", "status": "passed" },
    { "name": "authentication", "status": "passed" },
    { "name": "models", "status": "warning", "code": "llm.models_unsupported" },
    { "name": "completion", "status": "passed" },
    { "name": "streaming", "status": "passed" }
  ]
}
```

诊断顺序固定为：URL 解析与安全校验 → TCP/TLS/HTTP 可达 → 鉴权 → 模型列表 → 非流式最小生成 → 流式最小生成。模型列表失败是 warning，只要用户已填写模型且生成测试成功，整体连接仍可判定为可用。测试文本使用固定、无隐私内容的短字符串。

错误至少区分：DNS/连接拒绝、TLS、超时、401/403、404 路径错误、429、响应非 JSON、Chat Completions 结构不兼容、SSE 结构不兼容、模型列表不支持。

Profile 超时允许 1,000 至 120,000 ms，默认 8,000 ms。连接诊断不得自动重试；正常翻译保持现有上层重试和失败策略。

### 6.5 阶段二验收

- 五个预设只创建 `openai_compatible` Profile，不增加品牌 Provider 分支。
- LM Studio 和 Ollama 在空 API Key 下可保存、列模型、测试和翻译。
- OpenRouter 可配置 `HTTP-Referer`、`X-OpenRouter-Title` 等非敏感 Header。
- 模型列表失败时可输入模型并完成连接测试。
- 超时、认证失败、路径错误和 SSE 不兼容在 UI 中显示不同的可行动建议。
- 配置文件和日志中没有 API Key 或被禁止的敏感 Header。

## 7. 阶段三：翻译上下文、术语表与可编辑 Prompt

### 7.1 配置

将配置 schema 升级到 v13，扩展 `TranslationConfig`：

```rust
pub struct TranslationPromptConfig {
    pub system_prompt: String,
    pub context_enabled: bool,
    pub include_speaker: bool,
    pub include_microphone: bool,
    pub include_chatbox: bool,
    pub max_messages: u32,
    pub max_chars: u32,
    pub glossary: Vec<GlossaryEntry>,
}

pub struct GlossaryEntry {
    pub source: String,
    pub target: Option<String>,
    pub category: GlossaryCategory, // person | world | game | custom
    pub case_sensitive: bool,
}
```

默认值：

- `system_prompt` 等价于当前 `TRANSLATION_INSTRUCTIONS`。
- `context_enabled = false`，确保升级后不会在用户不知情时发送更多原文。
- 三个来源默认开启，但只有总开关开启后生效。
- `max_messages = 5`，允许范围 1 至 50。
- `max_chars = 4000`，允许范围 200 至 12000。
- `glossary = []`。

`system_prompt` 最大 8,000 字符；术语最多 500 条，单项原词和译词各最多 200 字符。`target = None` 表示保持原文，适用于人名、VRChat 世界名和不应翻译的游戏术语。

### 7.2 原文上下文仓库

新增 `core/src/db/translation_context.rs`，提供按时间合并的最近原文：

- `speaker` 和 `microphone` 从 `subtitles` 读取 `text`。
- `chatbox` 从 `chatbox_messages.original` 读取，不从 conversation subtitle 反推，保证 translation-only 发送模式仍使用用户原文。
- 只读取 final 原文，不保存或注入 ASR partial。
- 不读取 `subtitle_translations`，不把历史译文再次发送给 LLM。
- 先按来源过滤，再取最近 N 条，最后从最旧条目开始裁剪，直到格式化结果不超过 `max_chars`。
- 自动翻译当前字幕时排除当前记录，避免同一句既作为历史又作为待翻译文本。

上下文只在调用时从现有 SQLite 历史构造，不新增长期存储表，因此关闭功能后无需数据迁移或清理。

### 7.3 Prompt 构造

新增 Provider 无关的 `TranslationPromptBuilder`。它接收目标语言、可选源语言、术语表、历史原文和当前文本，输出：

- `instructions`：用户模板渲染后的系统指令，包含术语和历史区块。
- `input`：只包含当前待翻译文本及明确的源/目标语言元数据。

支持以下只读变量：

```text
{source_language}
{target_language}
{glossary}
{context}
```

当前文本不作为模板变量，始终通过 `LlmRequest.input` 单独传入，防止用户误删占位符后不再发送待翻译内容。未知变量在保存设置时返回稳定校验错误。上下文与术语使用明确分隔符，并保留“原文是数据，不是指令”的默认安全约束。

DeepL 和 Microsoft Translator 不消费 Prompt、上下文或术语；设置页根据能力注册表的 `supports_context` 只对 LLM Profile 显示这些控件。切换到专业翻译 API 时保留配置但不发送。

### 7.4 设置界面与隐私

翻译设置增加一个“LLM 翻译增强”区域：

- Prompt 多行编辑器、恢复默认和本地预览。
- 上下文总开关，以及扬声器、麦克风、Chatbox 三个独立来源开关。
- 最近消息数与最大字符数。
- 术语表编辑器，支持类别、固定译法或保持原文。
- 当前一次请求将附带的上下文条数和字符数预览。

开启上下文前显示隐私提示：最近原文会与当前文本一起发送给所选云端 Provider；本地 Profile 仍只发送到用户配置的本机地址。更新 `docs/privacy.md`，明确来源、默认关闭、长度限制和关闭方式。

### 7.5 阶段三验收

- 三种来源可以独立启用；关闭总开关后请求内容与阶段二一致。
- 上下文严格遵守 N 条与字符上限，顺序从旧到新，当前句不重复。
- Chatbox translation-only 消息注入的是 `original`，不是译文。
- 术语的固定译法和保持原文规则进入所有 LLM Provider 的 Prompt。
- DeepL/Microsoft 请求体不受上下文设置影响。
- 默认迁移不增加向云端发送的数据。
- Prompt 恢复默认后与当前固定 Prompt 的行为等价。

## 8. 阶段四：正式的第三方输出 API

### 8.1 独立服务边界

将配置 schema 升级到 v14，新增：

```rust
pub struct ExternalApiConfig {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub require_token: bool,
}
```

默认值为：`enabled = false`、`host = 127.0.0.1`、`port = 8767`、`require_token = false`。如果 `host` 不是回环地址，`require_token` 必须为 `true` 且必须已在凭据管理器中保存第三方 API token，否则拒绝启动该监听器。监听器在 Core 启动时创建，修改启用状态、地址或端口后需要重启应用；设置页必须明确标记这一点。

第三方 token 使用独立凭据目标 `VRCS/ExternalAPI/token`，不得复用内部 `session_token`。独立监听器只提供以下路由：

```http
GET /v1/health
GET /v1/capabilities
GET /v1/events        WebSocket upgrade
```

它不挂载设置、凭据、模型、数据库、Chatbox 或采集控制路由。现有内部 `/ws` 和 REST 鉴权保持不变。

### 8.2 事件信封

所有第三方事件使用统一信封：

```json
{
  "api_version": "1.0",
  "event_id": "550e8400-e29b-41d4-a716-446655440000",
  "type": "asr.final",
  "timestamp": "2026-08-14T10:30:45.123456Z",
  "message_id": "utterance-...",
  "source": "microphone",
  "payload": {}
}
```

字段语义：

- `event_id`：每个事件使用 UUID v4，保证唯一。
- `message_id`：同一次 utterance 的 partial/final 以及由该最终字幕触发的翻译事件保持一致。
- `timestamp`：事件产生时间的 UTC RFC 3339 字符串。
- `source`：`speaker`、`microphone` 或 `chatbox`。
- 数据库存储 ID 单独放在 payload 的 `subtitle_id`，不能替代跨生命周期的 `message_id`。

首期事件类型：

```text
asr.partial
asr.final
asr.failed
translation.started
translation.partial
translation.completed
translation.failed
chatbox.sent
```

`audio_level` 默认不向第三方 API 发布，避免高频事件挤占广播队列。若以后需要，作为单独订阅能力加入次版本。

### 8.3 订阅协议

连接成功后服务端先发送 `system.connected`，客户端必须在 5 秒内发送：

```json
{
  "type": "subscribe",
  "events": ["asr.*", "translation.completed", "translation.failed"]
}
```

服务端返回 `system.subscribed`，其中包含展开后的事件列表。未知事件模式返回 `system.error`，不静默忽略。客户端可在同一连接上再次发送 `subscribe` 替换订阅集合。服务端不接受其他命令。

Token 通过 WebSocket 握手的 `Authorization: Bearer` Header 传递；为了支持浏览器客户端，也允许 `Sec-WebSocket-Protocol` 中的受限 token 子协议。第三方正式文档不推荐 query token，避免令牌进入 URL、代理日志和崩溃报告。

### 8.4 消息关联改造

新增内部 `DomainEventHub`，由 ASR pipeline、`SubtitleLifecyclePublisher` 和 Chatbox 发送结果发布标准化领域事件。为保证 partial/final 关联：

- 云端 ASR 的现有 `utterance_id` 必须随 final 结果进入 `publish_text`。
- 本地 ASR 在开始处理语音段时生成 `message_id`。
- 保存字幕后，领域事件同时携带 `message_id` 与 `subtitle_id`。
- `TranslationJob` 保存 `message_id`，后续 started/partial/completed/failed 复用它。
- 现有 SQLite schema 首期不保存 `message_id`；该字段是实时事件关联标识，应用重启后不承诺可恢复。

内部桌面 `/ws` 可以继续从原广播通道发送旧事件，也可以由领域事件转换得到旧结构，但不得在阶段四中破坏现有前端协议。

### 8.5 背压与兼容策略

- 每个客户端使用有界队列；慢客户端队列满时先发送 `system.lagged`，随后关闭连接，避免拖慢 ASR 和翻译管线。
- API `1.x` 允许增加可选字段和新事件类型，不删除或改变已有字段语义。
- 删除字段、改名、改变必填性或事件语义必须发布 `/v2`。
- 任何外部序列化错误只影响对应客户端，不得使 Core、采集或翻译任务退出。

### 8.6 阶段四验收

- 默认不启动第三方监听器；启用后默认只监听 `127.0.0.1:8767`。
- 非回环地址在无 token 时无法启动。
- 客户端只能收到订阅的事件。
- 同一 utterance 的 partial、final 和翻译事件共享 `message_id`。
- 事件包含版本、唯一事件 ID、UTC 时间、来源和结构化 payload。
- 慢客户端不会阻塞或中断转写；断开后其他客户端继续工作。
- 第三方 token 不能访问内部 REST API，内部 session token 也不自动授权第三方 API。

## 9. 外部依赖与凭据

本方案不增加 Rust crate、npm package 或新的运行时。HTTP、SSE、WebSocket、UUID 和 JSON 处理继续使用现有的 `reqwest`、Axum、`uuid`、Tokio 与 Serde。

实施和人工验收可能用到以下凭据：

- Gemini API Key：由 Google AI Studio 创建，凭据管理器按 Profile 保存；环境变量依次读取 `VRCS_GEMINI_API_KEY` 和 `GEMINI_API_KEY`。
- OpenAI Compatible API Key：云端预设继续使用现有 `VRCS_OPENAI_COMPATIBLE_API_KEY` 或 Profile 凭据；LM Studio 与 Ollama 的默认 `auth_mode` 为 `none`。
- 第三方输出 API token：由用户在设置页生成或输入，保存在 `VRCS/ExternalAPI/token`；独立运行 Core 时可用 `VRCS_EXTERNAL_API_TOKEN` 覆盖，没有云端账号依赖。

离线单元测试使用本地 fixture 和测试服务器，不要求真实账号。只有人工端到端验收连接真实 Provider。

截至 2026-08-14，Gemini 原生接口文档可访问；实现时仍需对照当时的 Google 官方文档确认 API 版本和响应字段，没有确认前不能更新默认端点。

## 10. 文件级实施清单

预计会修改超过 8 个文件，并新增多个模块。应按阶段提交，不应把四个阶段合成一个大 PR。

### 阶段一

- `core/src/config.rs`：Gemini Provider 与 Profile 校验。
- `core/src/providers.rs`：Provider/预设/能力注册表。
- `core/src/credentials.rs`：Gemini 环境变量与凭据支持。
- `core/src/llm/mod.rs`、`core/src/llm/*.rs`：拆分协议与 Gemini 适配。
- `core/src/server/cloud.rs`、`core/src/server.rs`：Provider 元数据、模型列表与 Gemini 测试。
- `apps/desktop/src/types.ts`、`apps/desktop/src/api.ts`：能力响应类型。
- `apps/desktop/src/settings/api/ApiProfileEditor.tsx`：Gemini 与支持级别 UI。
- `apps/desktop/src/settings/sections/ApiManagementSettingsSection.tsx`：能力驱动展示。
- `apps/desktop/src/api-profile-purpose.ts`：删除品牌能力判断或缩减为纯类型辅助。
- 三份 locale JSON、README、architecture/privacy 文档和对应测试。

### 阶段二

- `core/src/providers.rs`：兼容预设和内部协议行为。
- `core/src/config.rs`、`core/src/config/migration.rs`：v12 Profile 字段、Header/auth/timeout 校验与迁移。
- `core/src/llm/openai_compatible.rs`：可选 Bearer、Header、Profile 超时、诊断探测。
- `core/src/server/cloud.rs`：结构化诊断响应与无 Key 模型查询。
- `apps/desktop/src/settings/api/ApiProfileEditor.tsx`：预设、鉴权、Header、超时表单。
- `apps/desktop/src/settings/useApiProfiles.ts`：诊断状态。
- API 管理 UI、locale、README、privacy 与测试。

### 阶段三

- `core/src/config.rs`、`core/src/config/migration.rs`：v13 Prompt/context/glossary。
- `core/src/db/translation_context.rs`、`core/src/db.rs`：最近原文查询。
- `core/src/translation/mod.rs` 或拆分后的 `prompt.rs`：Prompt Builder。
- `core/src/pipeline/dependencies.rs`、`core/src/server/translation.rs`：为自动、手动和预览翻译装配上下文。
- `apps/desktop/src/types.ts`、翻译设置组件、CSS、locale。
- `docs/privacy.md`、README 与对应测试。

### 阶段四

- `core/src/config.rs`、`core/src/config/migration.rs`：v14 External API 配置。
- `core/src/domain_events.rs`：稳定领域事件与消息关联。
- `core/src/external_api.rs`：独立 Axum 监听器、鉴权、订阅和背压。
- `core/src/lib.rs`：外部服务生命周期。
- `core/src/pipeline.rs`、`core/src/pipeline/dependencies.rs`：ASR message ID 贯穿。
- `core/src/subtitle_output.rs`、`core/src/translation/mod.rs`：翻译事件关联。
- `core/src/server/chatbox.rs`、`core/src/osc.rs`：Chatbox 发送事件。
- 系统设置 UI、locale、architecture/privacy/README 与协议文档 `docs/external-api-v1.md`。

## 11. 验证计划

每阶段完成后运行：

```powershell
cargo fmt --manifest-path core/Cargo.toml -- --check
cargo test --manifest-path core/Cargo.toml
npm --workspace apps/desktop test
npm run check:i18n
npm run build:frontend
```

阶段一额外覆盖：

- Gemini 请求 JSON、SSE 分块、空 chunk、错误响应和模型过滤的离线 fixture 测试。
- 所有 Provider 能力在 Rust API 与 TypeScript 解析后一致。
- 现有 v11 配置读取和保存不变。

阶段二额外覆盖：

- 有 Key 和无 Key 请求是否正确包含或省略 Authorization。
- Header 禁止列表、重复 Header、超时边界。
- v11 配置 fixture 无行为变化地迁移到 v12。
- `/models` 404 但 completion 成功时整体诊断为可用并带 warning。
- OpenAI Compatible SSE、普通 JSON 和错误 JSON 的兼容 fixture。

阶段三额外覆盖：

- 三来源组合、N 条限制、字符裁剪、时间排序、当前句排除。
- Chatbox 原文与译文的选择。
- Prompt 未知变量、长度上限、默认模板和术语转义。
- 上下文关闭时请求 fixture 与阶段二一致。

阶段四额外覆盖：

- 独立监听器回环/非回环安全校验。
- 订阅通配符、未知事件、5 秒超时、鉴权成功与失败。
- partial → final → translation 的 `message_id` 一致性。
- 广播 lag、客户端断开和 Core shutdown。
- 旧 `/ws` 前端解析测试不变。

人工验收至少使用：Gemini 云端、一个带 Key 的 OpenAI Compatible 服务、LM Studio 或 Ollama 中的一个无 Key 本地服务，以及一个示例 WebSocket 客户端。

## 12. 发布、回滚与风险

### 12.1 发布策略

- 每阶段单独 PR、单独发行说明；只有改变持久化配置结构的阶段升级 schema。
- 阶段一与阶段二先标记 Provider/预设为稳定；阶段三上下文默认关闭；阶段四第三方 API 默认关闭。
- 隐私文档必须与阶段三同版本发布，第三方协议文档必须与阶段四同版本发布。

### 12.2 回滚

- 每个迁移只增加带默认值的 JSON 字段，不改 SQLite DDL；代码回滚前需保留对更高 schema 的兼容读取，或明确要求恢复配置备份。
- 阶段三不新增上下文数据库，因此关闭开关即可恢复旧请求行为。
- 阶段四关闭 `external_api.enabled` 即可停止外部端口，不影响内部桌面协议。
- Gemini 和所有兼容预设都只是 Profile；删除或切换 Profile 不影响字幕历史。

### 12.3 主要风险

1. **兼容服务并不完全兼容。** 通过结构化诊断、手动模型名、协议行为与品牌预设分离来控制，不承诺支持任意“OpenAI Compatible”实现。
2. **上下文增加隐私暴露和 token 消耗。** 默认关闭、明确来源、双上限和发送预览是发布门槛。
3. **自定义 Header 可能泄露秘密。** 首期禁止常见鉴权 Header，只允许非敏感元数据；需要任意鉴权 Header 时应另行设计凭据存储，而不是放宽校验。
4. **第三方消费者会依赖事件结构。** 独立版本、契约测试和不复用内部 `/ws` 降低兼容成本。
5. **外部事件关联改动穿过 ASR、存储和翻译。** `message_id` 首期只存在于实时领域事件，不修改 SQLite，降低迁移与回滚风险。

本方案最脆弱的假设是：主流本地服务提供可用的 OpenAI Chat Completions 入口。若某个服务只提供品牌原生协议，应先判断其用户价值，再新增协议适配器；不能继续向 `openai_compatible` 中堆积 URL 或响应形状特判。

## 13. 参考资料

- [VRCT：LLM Translation Engines: Deep Dive & Customization](https://misyaguziya.github.io/VRCT-Docs/docs/feature-guide/llm-translation/)：Prompt 模板、Chat/Mic/Speaker 原文历史、消息数和字符数限制。
- [Google Gemini API reference](https://ai.google.dev/api)：原生鉴权和主要接口。
- [Google Gemini generateContent reference](https://ai.google.dev/api/generate-content)：`generateContent` 与 `streamGenerateContent` 请求和 SSE 响应。
- [Google Gemini OpenAI compatibility](https://ai.google.dev/gemini-api/docs/openai)：Google 明确建议非 OpenAI SDK 受限项目优先使用 Gemini 原生 API。
