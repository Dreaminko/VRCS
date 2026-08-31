<p align="center">
    <img src="apps/desktop/public/logos/VRCS_Logo.svg" width="50%" alt="標誌" />
</p>

# VRCS

[English](README.md) | [简体中文](README.zh-CN.md) | **繁體中文** | [日本語](README.ja-JP.md)

![](./screenshots/01.png)

VRCS 是一款專為 VRChat 設計的 Windows 即時字幕與語言學習工具。它能擷取系統輸出、VRChat 程序音訊及麥克風輸入，在桌面或 SteamVR 中顯示字幕，並運用這些字幕進行翻譯、字典查詢、學習分析、製作 Anki 卡片及輸出至 VRChat Chatbox。

[下載最新版本](https://github.com/Dreaminko/VRCS/releases/latest) · [回報問題](https://github.com/Dreaminko/VRCS/issues) · [參與貢獻](CONTRIBUTING.md)

[Discord](https://discord.gg/53H872eYq) · [QQ 群組](https://qm.qq.com/q/i9kOOxFn44)

## 下載與安裝

請從 [GitHub Releases](https://github.com/Dreaminko/VRCS/releases) 下載合適的安裝程式：

| 安裝程式 | 建議用途 | 額外需求 |
|---|---|---|
| `VRCS-<version>-windows-x64.exe` | 建議大多數使用者選用；支援雲端辨識及以本機 CPU 執行的 Whisper | 不需要 CUDA |
| `VRCS-<version>-windows-x64-CUDA.exe` | 使用 NVIDIA GPU 加速本機 Whisper | [CUDA 13.x Runtime](https://developer.nvidia.com/cuda-downloads?target_os=Windows)、cuBLAS，以及相容的 NVIDIA GPU 與驅動程式 |

標準版與 CUDA 版共用相同的設定、資料庫及模型目錄，因此切換版本時不必遷移資料。

執行環境需求：

- [Microsoft Visual C++ v14 Redistributable (x64)](https://aka.ms/vs/17/release/vc_redist.x64.exe)
- 首次啟動時需連上網際網路，以下載固定版本的 Silero VAD 模型；啟用語意斷句時，則會下載固定版本的 Smart Turn 模型
- 使用本機 Whisper 時，需先下載模型
- 使用雲端辨識、翻譯或學習分析時，需提供所選服務供應商的 API 認證資訊；供應商可能會收取費用

## 開始使用
首次啟動時會開啟設定精靈：

1. 選擇簡體中文、日文、英文或系統語言。
2. 選擇雲端即時辨識或本機 Whisper。
3. 設定系統音訊、VRChat 程序音訊及麥克風。
4. 測試麥克風並校準語音啟動門檻值。
5. 完成設定並開始轉錄。

你可以隨時在應用程式中變更設定。若要再次執行設定精靈，請開啟「**設定 → 系統**」。

## 功能

### 即時字幕與音訊

- Windows WASAPI 系統迴路音訊、專用 VRChat 程序迴路音訊及麥克風擷取
- 系統音訊與麥克風輸入雙串流轉錄，並可分別控制音訊來源及裝置
- Silero ONNX VAD；模型無法使用時，會自動改用能量式偵測
- 可選用 Smart Turn 語意斷句，讓語音片段的結束時機由本機控制
- 逐步更新的即時字幕、最終字幕、工作階段記錄及精簡視窗模式
- 本機 SQLite 字幕記錄，可依工作階段整理、重新命名及清除

### 語音辨識

- 本機 `whisper.cpp`，支援 CPU 及選用的 CUDA 加速
- 本機 Whisper 模型下載、完整性驗證、遷移及刪除
- Alibaba Cloud Qwen3 ASR 及 Fun-ASR 即時串流辨識
- OpenAI Realtime Transcription
- 雲端服務自動重新連線及可設定的失敗處理原則

### 翻譯與上下文

- 手動或自動翻譯
- DeepL、Microsoft Translator、OpenAI、Gemini 及 Alibaba Cloud LLM
- 相容 OpenAI Chat Completions 的服務，包括 DeepSeek、Groq、OpenRouter、LM Studio、Ollama 及自訂端點
- 自訂系統提示、本機術語表、線上術語表訂閱及近期字幕上下文
- 可選擇從本機 [VRCX-0](https://vrcx-0.dev/) 執行個體取得目前的世界名稱、成員顯示名稱及成員語言，以補充支援的 ASR 或 LLM 請求所需的上下文

### VRChat 與 SteamVR

- 將麥克風的最終字幕及譯文傳送至 VRChat OSC Chatbox
- Chatbox 快速輸入、翻譯預覽、格式設定及 144 字元限制處理
- 透過 OSCQuery 同步 VRChat 的 `MuteSelf` 狀態，並在靜音或狀態不明時阻止自動傳送
- SteamVR VR Overlay，提供頭戴裝置字幕及腕上對話檢視
- 可設定 Overlay 顯示原文、譯文或兩者；選擇系統音訊、麥克風及 Chatbox 來源；並調整位置、大小、不透明度及顯示時間

### 字典查詢、學習與 Anki

- 匯入及管理 Yomitan 字典套件
- 在字幕中選取單字進行查詢，同時保留原句及譯文上下文
- 透過獨立且明確的操作，針對所選字幕文字向設定的 AI 提問
- 從即時字幕、字幕記錄及查詢結果中收集學習素材
- 使用所選 LLM 提供符合上下文的詞義、分析句型及回顧對話
- 編輯詞彙卡、句型卡及克漏字卡草稿
- 選擇牌組、筆記類型及欄位對應後，透過 AnkiConnect 建立卡片

## 隱私權與資料

VRCS 不會儲存原始音訊。字幕記錄、工作階段、學習項目、字典及設定預設都儲存在本機。

使用本機 Whisper 時，語音不會傳送至雲端。使用雲端辨識時，偵測到的語音片段會傳送給所選的辨識服務供應商。使用雲端翻譯、學習分析或「問 AI」時，相關文字、使用者明確選取的上下文，以及送出的問題會傳送給相應的供應商。

## 從原始碼執行

開發環境需求：

- Windows 10 或 11
- Node.js 24+
- Rust 穩定版
- Visual Studio Build Tools，並安裝「**使用 C++ 的桌面開發**」工作負載
- 僅開發 CUDA 版本時需要 NVIDIA CUDA 13.x Toolkit，並須設定 `CUDA_PATH`

```powershell
npm install
npm run dev
```

預設指令使用 CPU 組建。若要啟用 CUDA：

```powershell
npm run dev:cuda
```

若只要執行獨立的 Rust Core：

```powershell
npm run dev:core
npm run dev:core:cuda
```

獨立 Core 預設監聽 `http://127.0.0.1:8766`，其字幕 WebSocket 位於 `ws://127.0.0.1:8766/ws`。桌面應用程式會自動產生並管理本機工作階段權杖。若你單獨執行 Core 並繫結至非回送位址，則必須明確設定非空白的 `VRCS_SESSION_TOKEN`。

## 測試

```powershell
npm run check:i18n
npm --workspace apps/desktop test
npm run build:frontend
.\scripts\test-core.ps1
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
```

## 組建發行版本

組建發行版本時，必須設定 `TAURI_SIGNING_PRIVATE_KEY` 及 `TAURI_UPDATER_PUBLIC_KEY`；若私密金鑰已加密，則需設定 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。請將私密金鑰存放在儲存庫之外，並妥善備份。

組建標準 Windows 安裝程式：

```powershell
npm run build
```

同時組建標準版及 CUDA 版：

```powershell
.\scripts\build-release.ps1 -Version 0.1.0 -IncludeCuda
```

## 參與貢獻

參與貢獻前，請先閱讀 [CONTRIBUTING.md](CONTRIBUTING.md)。若要新增或更新介面語言，請參閱 [LOCALIZATION.md](LOCALIZATION.md)。提交前請執行與變更相關的測試，且不要提交產生的組建成品。

## 授權條款

VRCS 採用 [GNU Affero General Public License v3.0](LICENSE)（`AGPL-3.0-only`）授權。第三方元件及其授權條款請參閱 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
