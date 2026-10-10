<p align="center">
    <img src="apps/desktop/public/logos/VRCS_Logo.svg" width="50%" alt="logo" />
</p>

# VRCS

[English](README.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-Hant.md) | **日本語**

![](./screenshots/01.png)

VRCSはVRChat向けのWindowsリアルタイム字幕・翻訳・言語学習ツールです。システム音声とマイク入力を取り込み、デスクトップやSteamVRに字幕を表示します。OCRで画面内の文字を翻訳し、字幕を辞書検索、学習分析、Ankiカード作成に使えます。

[ダウンロード](https://github.com/Dreaminko/VRCS/releases/latest) · [問題を報告](https://github.com/Dreaminko/VRCS/issues) · [コントリビュート](CONTRIBUTING.md) · [Discord](https://discord.gg/53H872eYq) · [QQグループ](https://qm.qq.com/q/i9kOOxFn44)

## インストール

[GitHub Releases](https://github.com/Dreaminko/VRCS/releases) から `VRCS-<version>-windows-x64.exe` をダウンロードしてください。

- Windows 10 / 11と [Microsoft Visual C++ v14 Redistributable（x64）](https://aka.ms/vs/17/release/vc_redist.x64.exe) が必要です。
- ローカルQwen ASRはCPUまたはVulkanに対応しています。認識設定からランタイムとモデルをダウンロードしてください。
- 初回はSilero VADモデルのダウンロードにインターネット接続が必要です。意味ベースの区切りを有効にするとSmart Turnモデルもダウンロードされます。
- クラウド認識、翻訳、学習分析には各プロバイダーのAPI認証情報が必要です。利用料金が発生する場合があります。
- OCRはローカルモデルをダウンロードするか、PaddleOCRクラウドのアクセストークンを設定してください。

## はじめに

1. セットアップウィザードで言語とローカルまたはクラウド認識を選択します。
2. システム音声、VRChatプロセス音声、マイクを選び、マイクのテストと音声トリガーのしきい値調整を行います。
3. 文字起こしを開始し、必要に応じて翻訳、Chatbox出力、SteamVR字幕を有効にします。

ウィザードは「設定 › システム」から再実行できます。クラウド認識は[Alibaba Cloud無料枠の入門ガイド（中国語）](./docs/AlibabaCloud_Free.md)も参照してください。

## 主な機能

- システム音声またはVRChatプロセス音声と、マイク入力のリアルタイム字幕。2系統の音声ソースの個別制御、コンパクトウィンドウ、ローカルのセッション履歴に対応。
- CPUまたはVulkanで動作するローカルQwen3 ASRと、Qwen3 ASR、Fun-ASR、OpenAI Realtime、Gemini、Groqによるクラウド認識。Silero VADと任意のSmart Turnによる音声区間の分割。
- DeepL、Microsoft Translator、OpenAI、Gemini、Alibaba Cloud LLM、OpenAI互換サービスによる手動・自動翻訳。システム音声とマイクにそれぞれ最大3つの翻訳先言語を設定し、言語ごとにサービスとモデルを選択可能。
- Qwen Live Translate、Gemini Live Translate（プレビュー）、OpenAI Realtime Translationによるリアルタイムの原文・訳文表示。各音声ソースの最初の翻訳先言語を使用し、Qwenは話者の区別にも対応。
- カスタムプロンプト、ローカル用語集、オンライン用語集の購読、直近の字幕コンテキスト。任意で [VRCX-0](https://vrcx-0.dev/) のワールド・メンバー情報を利用可能。言語プリセットで認識言語、翻訳先、Chatbox送信方法を保存・切り替え。
- 確定したマイク字幕と訳文のVRChat OSC Chatbox出力。クイック入力、翻訳プレビュー、多言語送信に対応。OSCQueryによるミュート同期を有効にすると、ミュート中や状態不明の場合は自動送信を停止。
- SteamVRのヘッドセット字幕と手首の会話ビュー。原文、訳文、多言語表示に対応し、ダッシュボードから言語、字幕の位置・サイズ・透明度、Chatbox設定を調整可能。
- デスクトップOCR：VRChatが前面にあるとき、設定可能なショートカットで文字範囲を選択。結果ウィンドウに原文と多言語の訳文を表示し、コピーや再認識が可能。
- VR OCR：ハンドジェスチャーやコントローラーで文字範囲を選び、手首パネルまたは両眼の原文位置に訳文を表示。
- PaddleOCRクラウド認識とローカルPP-OCRv6に対応。ローカルモデルは設定からダウンロードし、CPUまたはDirectML GPUで実行。
- Yomitan辞書のインポート、字幕の辞書検索、「AIに質問」、学習素材の収集、語義説明、文型分析、会話レビュー。単語・文型・穴埋めカードの下書きを編集し、AnkiConnect経由でカードを作成。

## プライバシー

VRCSは元の音声を保存しません。字幕履歴、学習項目、辞書、設定はデフォルトでローカルに保存されます。ローカルQwen ASRとローカルOCRは端末上で音声と画像を処理し、クラウド認識は音声区間や選択した画像をプロバイダーに送信します。

翻訳、学習分析、「AIに質問」は、関連テキスト、明示的に選択したコンテキスト、送信した質問を設定済みのサービスに送ります。ローカル認識後のテキストがクラウドに送られるかどうかは、選択した翻訳・AIサービスによります。

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
