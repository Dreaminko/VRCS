<p align="center">
    <img src="apps/desktop/public/logos/VRCS_Logo.svg" width="50%" alt="logo" />
</p>

<p align="center">
    VRCSはVRChat向けの字幕・翻訳・言語学習・AIツールです。
</p>

<p align="center">
    <img src="https://img.shields.io/github/v/release/Dreaminko/VRCS" alt="GitHub Release" />
    <img src="https://img.shields.io/github/license/Dreaminko/VRCS" alt="GitHub License" />
    <img src="https://img.shields.io/github/stars/Dreaminko/VRCS?style=flat" alt="GitHub Repo stars" />
</p>

<p align="center">
    <a href="README.md">English</a> | <a href="README.zh-CN.md">简体中文</a> | <a href="README.zh-Hant.md">繁體中文</a> | <strong>日本語</strong>
</p>

<p align="center">
    <a href="https://github.com/Dreaminko/VRCS/releases/latest">ダウンロード</a> · <a href="https://github.com/Dreaminko/VRCS/issues">問題を報告</a> · <a href="CONTRIBUTING.md">コントリビュート</a> · <a href="https://discord.gg/53H872eYq">Discord</a> · <a href="https://qm.qq.com/q/i9kOOxFn44">QQグループ</a>
</p>

![](./screenshots/01.png)

## インストール

[GitHub Releases](https://github.com/Dreaminko/VRCS/releases) から `VRCS-<version>-windows-x64.exe` をダウンロードしてください。

- Windows 10 / 11と [Microsoft Visual C++ v14 Redistributable（x64）](https://aka.ms/vs/17/release/vc_redist.x64.exe) が必要です。
- ローカルQwen ASRはCPUまたはVulkanに対応しています。認識設定からランタイムとモデルをダウンロードしてください。
- 初回はSilero VADモデルのダウンロードにインターネット接続が必要です。意味ベースの区切りを有効にするとSmart Turnモデルもダウンロードされます。
- クラウド認識、翻訳、学習分析には各プロバイダーのAPI認証情報が必要です。利用料金が発生する場合があります。
- OCRはローカルPP-OCRv6モデルをダウンロードしてください。

## はじめに

1. セットアップウィザードで言語とローカルまたはクラウド認識を選択します。
2. システム音声、VRChatプロセス音声、マイクを選び、マイクのテストと音声トリガーのしきい値調整を行います。
3. 文字起こしを開始し、必要に応じて翻訳、Chatbox出力、SteamVR字幕を有効にします。

ウィザードは「設定 › システム」から再実行できます。

「設定 › システム › 機能の選択」で使わない機能をオフにすると、設定とデータを保持したまま、関連する設定と操作を非表示にできます。

## 主な機能

- システム音声またはVRChatプロセス音声とマイクから、2系統のリアルタイム字幕を生成。音声ソースの個別制御、コンパクトウィンドウ、ローカルのセッション履歴に対応。
- 終了した会話のAI要約タイトルを生成し、タイトルで履歴を検索。「設定 › システム › 会話管理」でモデルを設定し、自動生成を有効にできます。デフォルトは無効です。新しい会話の作成で前の会話が終了し、文字起こしの停止は一時停止として扱います。履歴のメニューから手動で生成・再生成でき、手動の名前を優先します。
- CPUまたはVulkanで動作するローカルQwen3 ASRと、Qwen3 ASR、Fun-ASR、OpenAI Realtime、Gemini、Groq、OpenRouterなどのクラウド認識に対応。Silero VADと任意のSmart Turnで音声区間を分割。
- 音声ソースごとに最大3言語へ手動・自動翻訳し、言語ごとにサービスとモデルを選択可能。DeepL、Microsoft Translator、OpenAI、Gemini、Alibaba Cloud LLM、OpenAI互換サービスに対応。
- Qwen Live Translate、Gemini Live Translate、OpenAI Realtime Translationでリアルタイムの原文と訳文を表示。
- プロンプト、ローカル・購読用語集、字幕コンテキストをカスタマイズ。[VRCX-0](https://vrcx-0.dev/) のワールド・メンバー情報も利用でき、言語プリセットで認識・翻訳・Chatbox設定を切り替え可能。
- マイク字幕と訳文をVRChat OSC Chatboxに送信。クイック入力、翻訳プレビュー、多言語出力、任意のOSCQueryミュート同期に対応。
- SteamVRのヘッドセットや手首に多言語字幕を表示。ダッシュボードから字幕のレイアウト、言語、Chatbox設定を調整可能。
- デスクトップやVRで文字を範囲選択してOCR翻訳。ローカルPP-OCRv6（CPU / DirectML GPU）を使い、結果をデスクトップウィンドウ、手首パネル、VRの原文位置に表示可能。

https://github.com/user-attachments/assets/aa57a9d3-def5-47ad-ba5b-2af792f54349

- デスクトップやVRで選択した画像について質問。デスクトップでは文字・音声入力、VRでは左トリガーを押しながら音声入力。回答は専用ウィンドウやMarkdown対応の手首パネルに表示され、OCRは不要。

https://github.com/user-attachments/assets/2c898c18-ffa3-4c00-849e-b7c00a182a2a

- Yomitan辞書をインポートし、字幕から辞書検索や「AIに質問」、語義説明、文型分析、会話レビューを利用可能。単語・文型・穴埋めカードを編集し、AnkiConnect経由でAnkiに送信。

## プライバシー

VRCSは元の音声を保存しません。字幕履歴、学習項目、辞書、設定はデフォルトでローカルに保存されます。ローカルQwen ASRとローカルOCRは端末上で音声と画像を処理し、クラウド音声認識は音声区間をプロバイダーに送信します。

翻訳、学習分析、「AIに質問」は、関連テキスト、明示的に選択したコンテキスト、送信した質問を設定済みのサービスに送ります。ローカル認識後のテキストがクラウドに送られるかどうかは、選択した翻訳・AIサービスによります。

会話タイトルの生成では、その会話の字幕原文の一部または全部を選択したモデルサービスに送信し、料金が発生する場合があります。訳文、元の音声、他の会話は送信しません。長い会話はサンプリングし、過去の会話を自動で一括処理しません。

デスクトップOCRと画像質問応答は、選択範囲のみを処理します。画像質問応答は選択した画像と質問文を設定済みのモデルサービスに送信し、クラウド音声認識を使う場合は質問の音声も送信します。画像と問答はデフォルトでディスクや問答履歴に保存されません。音声認識と画像リクエストは個別に課金される場合があります。

## 開発

Windows 10 / 11、Node.js 24+、Rustup（バージョンとコンポーネントは `rust-toolchain.toml` を参照）、Visual Studio Build Toolsの「C++ によるデスクトップ開発」ワークロード、`PATH` に追加したCMakeが必要です。

リポジトリのルートでPowerShellを開き、実行します。

```powershell
npm install
npm run dev
```

デフォルトのビルドはローカルQwen ASRにCPUを使用します。Vulkanアクセラレーションには `npm run dev:vulkan` を使います。ローダーは自動準備され、GPU SDKやシェーダーコンパイラーは不要です。Qwenランタイムとモデルは認識設定からダウンロードします。独立したバックエンドの開発は [Core README](core/README.md) を参照してください。

変更に関連するチェックを実行します。

```powershell
npm run check:i18n
npm --workspace apps/desktop test
npm run build:frontend
.\scripts\check-rust.ps1
```

Rustスクリプトは両crateのフォーマット、Clippy、テストを実行します。Vulkan関連の変更には `-Vulkan` を追加してください。

CPUとVulkanに対応するWindowsインストーラーをビルドします。

```powershell
npm run build
```

貢献方法は [CONTRIBUTING.md](CONTRIBUTING.md)、インターフェースの翻訳は [LOCALIZATION.md](LOCALIZATION.md) を参照してください。

## ライセンス

[GNU Affero General Public License v3.0](LICENSE)（`AGPL-3.0-only`）。サードパーティのライセンスは [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) を参照してください。
