# VRChat 实时字幕学习工具：基础实现任务书

## 一、项目目标

开发一个开源桌面软件，用于实时捕获 VRChat 中的对话音频，通过本地 Whisper 转写生成字幕，并支持基础查词与 Anki 制卡。

项目核心定位：

> 本地实时转写 VRChat 对话，并将真实语境中的单词和句子转化为可复习的 Anki 卡片。

本阶段目标是完成基础可运行版本，重点打通以下主链路：

```text
系统音频捕获
→ VAD 人声检测
→ faster-whisper 本地转写
→ 字幕实时显示
```

暂不要求实现完整 VR Overlay，但架构需要为后续 OpenVR Overlay 预留接口。

---

## 二、推荐技术栈

### 桌面端

* Tauri v2
* TypeScript
* SvelteKit 或 React
* WebSocket / HTTP 与 Python sidecar 通信

### 核心后端

* Python 3.11 或 3.12
* faster-whisper
* CTranslate2
* PyAudioWPatch
* Silero VAD
* SQLite + FTS5
* FastAPI 或 aiohttp
* WebSocket 实时推送字幕

### 词典系统

基础阶段：

* SQLite 内置测试词典
* 简单英文词条查询
* 简单日文词条查询预留接口

后续扩展：

* Yomitan 字典格式
* StarDict
* MDict
* AnkiConnect
* HTTP POST 调用本地 AnkiConnect API

### 后续 VR 支持

* OpenVR IVROverlay
* SteamVR Overlay
* Overlay 作为独立模块连接同一个 core-python 服务


---

## 四、核心模块要求

## 1. Python Core Service

Python core 是整个项目的核心服务，负责音频、VAD、ASR、字幕、词典和 Anki。

### 必须提供的服务能力

* 启动本地 HTTP 服务
* 启动 WebSocket 服务
* 枚举音频输入 / 输出设备
* 捕获系统扬声器音频
* 对音频进行 VAD 检测
* 调用 faster-whisper 转写
* 生成字幕片段
* 向前端实时推送字幕
* 查询本地词典
* 调用 AnkiConnect 创建卡片

### 推荐服务端口

```text
HTTP API:       http://127.0.0.1:8765
WebSocket API:  ws://127.0.0.1:8765/ws
```

---

## 2. 音频捕获模块


### 要求

* 支持 Windows WASAPI Loopback
* 可以捕获系统输出音频
* 允许用户选择音频设备
* 如果没有选择设备，默认使用系统默认输出设备
* 音频采样率建议统一为 16000 Hz 或 48000 Hz 后重采样到 16000 Hz
* 输出 PCM float32 或 int16 音频块

### 注意事项

VRChat 中其他玩家的声音来自系统输出，不是麦克风。因此基础版本的主要音频来源应是扬声器 loopback。

---

## 3. VAD 人声检测模块

路径：

```text
core-python/app/vad/
```

### 功能要求

使用 Silero VAD 判断音频中是否存在人声。

### 基本逻辑

```text
音频块
→ VAD 判断
→ 如果有人声，加入 speech buffer
→ 如果静音超过阈值，提交一个 speech segment
```

---

## 4. ASR 转写模块

路径：

```text
core-python/app/asr/
```

### 使用技术

* faster-whisper
* 默认模型：small
* 可配置模型：tiny / base / small / medium / large-v3
* 默认设备：auto
* 可配置 device：cpu / cuda
* 可配置 compute_type：int8 / int8_float16 / float16

### 功能要求


### 语言设置

* 支持 auto
* 支持手动指定语言
* 优先支持：

  * English
  * Japanese
  * Chinese
  * Korean
  * Spanish
  * French
  * German
---

## 5. 字幕模块


### 功能要求

* 保存最近字幕历史
* 默认保留最近 500 条
* 支持前端查询历史字幕
* 支持通过 WebSocket 实时推送字幕
* 支持点击字幕进行查词和制卡

---

### 后续扩展预留

需要预留 importer 接口：

```python
class DictionaryImporter:
    def import_file(self, path: str) -> None:
        pass
```

未来支持：

```text
Yomitan zip
StarDict
MDict
WordNet
Wiktionary / Kaikki
```
---

## 10. 桌面前端

路径：

```text
apps/desktop/
```

### 页面结构

至少实现以下界面：

```text
1. 主字幕页
2. 字幕历史页
5. ASR 设置页
6. 音频设备设置页
```

### 主字幕页

需要显示：

* 当前转写状态
* 当前使用的音频设备
* 当前 Whisper 模型
* 当前识别语言
* 最近字幕
* 开始 / 停止按钮

### 字幕显示

每条字幕显示：

```text
[时间] 字幕文本
```


### ASR 设置页

需要支持：

* 选择 Whisper 模型
* 选择语言
* 选择 device：auto / cpu / cuda
* 选择 compute_type：int8 / float16 / int8_float16
* 显示模型加载状态

### 音频设备设置页

需要支持：

* 枚举音频设备
* 选择系统输出设备
* 测试音频捕获状态

---

## 五、数据库设计

使用 SQLite。

---

## 六、配置文件

需要支持本地配置文件。

---

## 七、隐私与安全要求

本项目默认应遵守以下原则：

```text
1. 默认本地处理音频
2. 默认不上传音频
3. 默认不保存原始音频
4. 默认只保存字幕文本
5. 用户必须明确开启音频保存功能
6. 日志中不要保存完整音频数据
7. 提醒用户遵守 VRChat 社区规则与当地隐私法律
```

需要在 `docs/privacy.md` 中说明：

* 本软件如何捕获音频
* 本软件默认不会上传音频
* 使用 AnkiConnect 时会向本地 Anki 写入卡片
* 如果用户手动启用云端功能，需明确提示

---

## 八、基础完成标准

当以下功能可运行时，认为基础开发完成：

### Core Service

* 可以启动 Python core service
* `/health` 正常返回
* 可以列出音频设备
* 可以启动 / 停止音频捕获
* 可以使用 VAD 切分人声
* 可以调用 faster-whisper 转写音频
* 可以通过 WebSocket 推送字幕

### Desktop UI

* Tauri 桌面端可以启动
* 可以连接 Python core
* 可以显示实时字幕
* 可以显示字幕历史

Anki制卡以及词典查询在初期暂不完成，但是要预留接口。

---

## 九、开发顺序

请按以下顺序开发：

```text
1. 初始化 monorepo 项目结构
2. 创建 Python core service
3. 实现 /health
4. 实现配置文件读取
5. 实现 SQLite 初始化
6. 实现音频设备枚举
7. 实现系统音频捕获
8. 实现 VAD 切分
9. 接入 faster-whisper
10. 实现字幕数据结构和历史存储
11. 实现 WebSocket 字幕推送
12. 创建 Tauri 桌面端
13. 实现前端连接 core service
14. 实现实时字幕显示
15. 实现字幕历史页
16. 编写 README 和基础开发文档
```

---

## 十、暂不实现的内容

基础阶段暂不要求实现：

```text
1. 完整 VR Overlay
2. VR 控制器划词
3. 多说话人识别
4. 音频说话人分离
5. 云端 ASR
6. 自动翻译
7. 完整 Yomitan 导入
8. 完整 StarDict / MDict 导入
9. 自动生成 cloze 卡
10. 自动保存音频片段
```

但需要在架构中为这些功能预留扩展点。

---

## 十一、代码质量要求

### Python

* 使用类型标注
* 使用 dataclass 或 pydantic model 定义数据结构
* 异步任务需要清晰管理
* 音频捕获、ASR、WebSocket 推送之间不要互相阻塞
* 出错时返回明确错误信息
* 避免把业务逻辑写进 API route 中

### TypeScript

* 使用明确类型
* WebSocket 状态需要可视化
* API 请求需要统一封装
* UI 状态和后端状态分离
* 组件拆分清晰

### 文档

至少编写：

```text
README.md
docs/architecture.md
docs/development.md
docs/privacy.md
docs/roadmap.md
```

---

## 十二、README 内容要求

README 至少包含：

```text
1. 项目名称
2. 项目简介
3. 功能特点
4. 技术栈
5. 项目状态
6. 安装方式
7. 开发环境启动方式
8. AnkiConnect 使用说明
9. 隐私说明
10. Roadmap
11. License
```

项目简介建议：

```text
A local-first VRChat subtitle mining tool that captures system audio, transcribes conversations with faster-whisper, provides instant dictionary lookup, and creates Anki cards from real conversation context.
```

中文简介：

```text
一个本地优先的 VRChat 字幕挖矿工具，可捕获系统音频，使用 faster-whisper 实时转写对话，支持即时查词，并将真实对话语境制作成 Anki 卡片。
```

---

该项目名称暂定为VRCS。


---

## 十四、最终交付物

请交付以下内容：

```text
1. 可运行的 monorepo 项目
2. Python core service
3. Tauri desktop app
4. 基础音频捕获功能
5. faster-whisper 转写功能
6. WebSocket 字幕推送
7. 字幕显示 UI
8. 内置测试词典查词
9. AnkiConnect 制卡
10. SQLite 数据存储
11. 基础 README 和 docs
```

完成后，用户应能够：

```text
1. 启动 Python core
2. 启动桌面端
3. 选择系统音频输出设备
4. 开始转写
5. 在桌面端看到实时字幕
6. 选中字幕中的词语
7. 查看释义
8. 一键创建 Anki 卡片
```
