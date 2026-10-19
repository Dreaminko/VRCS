<p align="center">
    <img src="apps/desktop/public/logos/VRCS_Logo.svg" width="50%" alt="標誌" />
</p>

<p align="center">
    VRCS 是面向 VRChat 的字幕、翻譯、語言學習與 AI 工具。
</p>

<p align="center">
    <img src="https://img.shields.io/github/v/release/Dreaminko/VRCS" alt="GitHub Release" />
    <img src="https://img.shields.io/github/license/Dreaminko/VRCS" alt="GitHub License" />
    <img src="https://img.shields.io/github/stars/Dreaminko/VRCS?style=flat" alt="GitHub Repo stars" />
</p>

<p align="center">
    <a href="README.md">English</a> | <a href="README.zh-CN.md">简体中文</a> | <strong>繁體中文</strong> | <a href="README.ja-JP.md">日本語</a>
</p>

<p align="center">
    <a href="https://github.com/Dreaminko/VRCS/releases/latest">下載</a> · <a href="https://github.com/Dreaminko/VRCS/issues">回報問題</a> · <a href="CONTRIBUTING.md">參與貢獻</a> · <a href="https://discord.gg/53H872eYq">Discord</a> · <a href="https://qm.qq.com/q/i9kOOxFn44">QQ 群組</a>
</p>

![](./screenshots/01.png)

## 安裝

從 [GitHub Releases](https://github.com/Dreaminko/VRCS/releases) 下載 `VRCS-<version>-windows-x64.exe`。

- Windows 10 / 11，需安裝 [Microsoft Visual C++ v14 Redistributable（x64）](https://aka.ms/vs/17/release/vc_redist.x64.exe)。
- 本機 Qwen ASR 支援 CPU 或 Vulkan，請在辨識設定中下載執行元件及模型。
- 首次使用需連網下載 Silero VAD 模型；啟用語意斷句時還需下載 Smart Turn 模型。
- 雲端辨識、翻譯及學習分析需要對應服務供應商的 API 認證資訊，供應商可能收取費用。
- OCR 需下載本機 PP-OCRv6 模型。

## 開始使用

1. 按設定精靈選擇語言，以及本機或雲端辨識。
2. 選擇系統音訊、VRChat 程序音訊或麥克風，測試麥克風並調整語音啟動門檻值。
3. 開始轉錄，按需啟用翻譯、Chatbox 輸出或 SteamVR 字幕。

可從「設定 › 系統」重新執行精靈。

可在「設定 › 系統 › 功能選項」關閉不使用的功能，隱藏相關設定及入口，保留已有設定與資料。

## 主要功能

- 擷取系統或 VRChat 程序音訊與麥克風，產生雙串流即時字幕，支援獨立音源控制、精簡視窗及本機工作階段記錄。
- 為已結束的對話產生 AI 摘要標題，並依標題尋找記錄。可在「設定 › 系統 › 對話管理」設定模型並啟用自動產生，預設關閉；新增對話會結束舊對話，停止轉寫僅暫停。也可從歷史對話選單手動產生或重新產生，自訂名稱始終優先。
- 本機 Qwen3 ASR 支援 CPU 或 Vulkan；雲端辨識支援 Qwen3 ASR、Fun-ASR、OpenAI Realtime、Gemini、Groq 及 OpenRouter 等服務，搭配 Silero VAD 與可選的 Smart Turn 進行語音分段。
- 每路音源最多翻譯為三種語言，可分別選擇服務與模型，支援手動或自動翻譯，以及 DeepL、Microsoft Translator、OpenAI、Gemini、Alibaba Cloud LLM 及相容 OpenAI 的服務。
- Qwen Live Translate、Gemini Live Translate 及 OpenAI Realtime Translation 直接產生即時原文與譯文。
- 自訂提示詞、本機與訂閱術語表、字幕上下文，可串接 [VRCX-0](https://vrcx-0.dev/) 的世界與成員資訊，透過語言預設切換辨識、翻譯及 Chatbox 設定。
- 將麥克風字幕及譯文傳送至 VRChat OSC Chatbox，支援快速輸入、翻譯預覽、多語言輸出及可選的 OSCQuery 靜音同步。
- 在 SteamVR 頭戴裝置或手腕上閱讀多語言字幕，透過儀表板調整字幕版面、語言及 Chatbox 設定。
- 在桌面或 VR 中框選文字進行 OCR 翻譯，使用本機 PP-OCRv6（CPU / DirectML GPU），結果可顯示在桌面視窗、手腕面板或 VR 原文位置。
- 對桌面或 VR 中的選區圖片提問：桌面支援鍵入或語音輸入，VR 中按住左扳機說話。回答顯示在獨立視窗或支援 Markdown 的手腕面板上，不需啟用 OCR。
- 匯入 Yomitan 字典，在字幕中查詞或「問 AI」，進行詞義解釋、句型分析及對話回顧；編輯詞彙、句型或克漏字卡片，透過 AnkiConnect 傳送至 Anki。

## 隱私

VRCS 不儲存原始音訊，字幕記錄、學習項目、字典及設定預設儲存在本機。本機 Qwen ASR 與本機 OCR 在裝置上處理語音及影像；雲端語音辨識將語音片段傳送給所選供應商。

翻譯、學習分析及「問 AI」會將相關文字、明確選擇的上下文及提交的問題傳送給設定的服務。本機辨識後的文字是否傳送至雲端，取決於所選翻譯及 AI 服務。

對話標題生成會將該對話的部分或全部字幕原文傳送至所選模型服務，可能產生費用；不傳送譯文、原始音訊或其他對話。長對話會取樣，舊對話不會自動批次補全。

桌面 OCR 與視覺問答僅處理框選區域。視覺問答將選區圖片及問題文字傳送給設定的模型服務；使用雲端語音辨識時，還會傳送本次問題的語音。圖片及問答預設不儲存至磁碟或問答記錄，語音辨識與圖像請求可能分別計費。

## 開發

需要 Windows 10 / 11、Node.js 24+、Rustup（版本及元件見 `rust-toolchain.toml`）、Visual Studio Build Tools 的「使用 C++ 的桌面開發」工作負載，以及已加入 `PATH` 的 CMake。

在儲存庫根目錄的 PowerShell 中執行：

```powershell
npm install
npm run dev
```

預設建置使用 CPU 執行本機 Qwen ASR。Vulkan 加速使用 `npm run dev:vulkan`，命令會自動準備載入器，不需 GPU SDK 或著色器編譯器。Qwen 執行元件及模型在辨識設定中下載。獨立後端開發見 [Core README](core/README.md)。

依變更範圍執行檢查：

```powershell
npm run check:i18n
npm --workspace apps/desktop test
npm run build:frontend
.\scripts\check-rust.ps1
```

Rust 指令碼對兩個 crate 執行格式檢查、Clippy 及測試；Vulkan 相關變更新增 `-Vulkan`。

建置支援 CPU 及 Vulkan 的 Windows 安裝程式：

```powershell
npm run build
```

貢獻指南見 [CONTRIBUTING.md](CONTRIBUTING.md)，介面翻譯指南見 [LOCALIZATION.md](LOCALIZATION.md)。

## 授權

[GNU Affero General Public License v3.0](LICENSE)（`AGPL-3.0-only`）。第三方授權見 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
