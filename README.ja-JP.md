<p align="center">
    <img src="apps/desktop/public/logos/VRCS_Logo.svg" width="50%" alt="logo" />
</p>

# VRCS

[English](README.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-Hant.md) | **日本語**

![](./screenshots/01.png)

VRCS は、VRChat 向けの Windows リアルタイム字幕・言語学習ツールです。システム出力、VRChat プロセスの音声、マイク入力を取り込み、デスクトップまたは SteamVR に字幕を表示します。さらに、その字幕を翻訳、辞書検索、学習分析、Anki カード作成、VRChat Chatbox への出力に活用できます。

[最新版をダウンロード](https://github.com/Dreaminko/VRCS/releases/latest) · [問題を報告](https://github.com/Dreaminko/VRCS/issues) · [コントリビュート](CONTRIBUTING.md)

[Discord](https://discord.gg/53H872eYq) · [QQ グループ](https://qm.qq.com/q/i9kOOxFn44)

## ダウンロードとインストール

[GitHub Releases](https://github.com/Dreaminko/VRCS/releases) から `VRCS-<version>-windows-x64.exe` をダウンロードしてください。この標準インストーラーは、クラウド認識と、アプリが管理するローカル Qwen ASR に対応し、CPU または Vulkan で実行できます。

インストーラーには Vulkan ローダーが含まれています。Vulkan アクセラレーションには互換性のある GPU とグラフィックスドライバーが必要ですが、Vulkan SDK や CUDA は不要です。管理されたローカル認識を開始する前に、認識設定から Qwen ランタイムとモデルをダウンロードしてください。クラウド認識にはこのランタイムは不要です。

旧標準版と旧 CUDA 版のクライアントは、更新機能から同じ標準インストーラーを取得します。旧ローカル Whisper の選択は管理された Qwen に移行され、クラウド接続に失敗した場合は再接続を続け、Whisper にはフォールバックしません。既存の Whisper ファイルはディスク上に残ります。旧版で不要になったことを確認してから手動で削除してください。同じディレクトリにある Qwen パッケージは残してください。既存の Qwen パッケージ、モデルディレクトリ、設定はそのまま保持されます。

動作要件：

- [Microsoft Visual C++ v14 Redistributable（x64）](https://aka.ms/vs/17/release/vc_redist.x64.exe)
- 初回起動時に、固定バージョンの Silero VAD モデルをダウンロードするためのインターネット接続。意味ベースの区切りを有効にすると、固定バージョンの Smart Turn モデルもダウンロードされます
- 管理されたローカル Qwen ASR を使用する場合は、モデルとランタイムの初回ダウンロード
- クラウド認識、翻訳、学習分析を使用する場合は、選択したサービスプロバイダーの API 認証情報。プロバイダーによって料金が発生する場合があります

## はじめに
初回起動時にセットアップウィザードが開きます。

1. 簡体字中国語、日本語、英語、またはシステム言語を選択します。
2. クラウドのリアルタイム認識または管理されたローカル Qwen ASR を選択します。
3. システム音声、VRChat プロセス音声、マイクを設定します。
4. マイクをテストし、音声トリガーのしきい値を調整します。
5. セットアップを完了し、文字起こしを開始します。

設定はアプリ内でいつでも変更できます。セットアップウィザードは「設定 → システム」から再実行できます。

## 主な機能

### リアルタイム字幕と音声

- Windows WASAPI システムループバック、VRChat プロセス専用ループバック、マイクキャプチャ
- システム音声とマイクの2系統文字起こし。音声ソースとデバイスを個別に制御可能
- Silero ONNX VAD。モデルを利用できない場合はエネルギーベース検出へ自動的にフォールバック
- ローカルで終了時機を制御できる音声区間向けの、任意の Smart Turn 意味ベース区切り
- リアルタイムの逐次字幕、確定字幕、セッション履歴、コンパクトウィンドウモード
- 字幕履歴をローカル SQLite に保存し、セッション単位で整理、名前変更、削除が可能

### 音声認識

- アプリが管理するローカル Qwen3 ASR。モデルのダウンロード、整合性検証、CPU または Vulkan での実行に対応
- Alibaba Cloud Qwen3 ASR と Fun-ASR によるリアルタイムストリーミング認識
- OpenAI Realtime Transcription
- クラウドサービスの自動再接続

### 翻訳とコンテキスト

- 手動翻訳または自動翻訳
- DeepL、Microsoft Translator、OpenAI、Gemini、Alibaba Cloud LLM
- DeepSeek、Groq、OpenRouter、LM Studio、Ollama、カスタムエンドポイントを含む OpenAI 互換 Chat Completions サービス
- カスタムシステムプロンプト、ローカル用語集、オンライン用語集の購読、直近の字幕コンテキスト
- ローカルの [VRCX-0](https://vrcx-0.dev/) から現在のワールド名、メンバーの表示名、使用言語を任意で読み取り、対応する ASR または LLM リクエストにコンテキストを追加

### VRChat と SteamVR

- 自分のマイクから生成した確定字幕と翻訳を VRChat OSC Chatbox に送信
- Chatbox のクイック入力、翻訳プレビュー、書式設定、144文字制限への対応
- OSCQuery を介して VRChat の `MuteSelf` 状態を同期し、ミュート中または状態不明の場合は自動送信を停止
- SteamVR VR Overlay：ヘッドセット内字幕と手首装着型の会話ビュー
- VRCS の実行中は SteamVR ダッシュボード下部バーからクイック設定を表示
- Overlay には原文、翻訳、または両方を表示可能。システム音声、マイク、Chatbox の各ソースを選択し、位置、サイズ、透明度、表示時間を調整可能

### 辞書検索、学習、Anki

- Yomitan 辞書パッケージのインポートと管理
- 字幕内の単語を選択して検索し、元の文と翻訳のコンテキストを保持
- 独立した「AI に質問」操作から、選択した字幕テキストについて設定済みモデルに質問
- リアルタイム字幕、字幕履歴、検索結果から学習素材を収集
- 選択した LLM を使用した文脈に応じた語義説明、文型分析、会話レビュー
- 単語カード、文型カード、穴埋めカードの下書きを編集
- デッキ、ノートタイプ、フィールドマッピングを選択し、AnkiConnect 経由でカードを作成

## プライバシーとデータ

VRCS は元の音声を保存しません。字幕履歴、セッション、学習項目、辞書、設定はデフォルトでローカルに保存されます。

アプリが管理するローカル Qwen ASR を使用する場合、音声はクラウドに送信されません。クラウド認識を使用する場合、検出された音声区間が選択した認識サービスプロバイダーに送信されます。クラウド翻訳、学習分析、または「AI に質問」を使用する場合、関連テキスト、ユーザーが明示的に選択したコンテキスト、送信した質問が対応するプロバイダーに送信されます。

## ソースから実行

開発環境：

- Windows 10 / 11
- Node.js 24+
- Rustup と `rust-toolchain.toml` で指定された Rust バージョンおよびコンポーネント
- Visual Studio Build Tools と「C++ によるデスクトップ開発」ワークロード
- `PATH` に追加された CMake

リポジトリのルートで PowerShell を開き、次のコマンドを実行します。

```powershell
npm install
npm run dev
```

デフォルトの開発ビルドはクラウド機能と、管理されたローカル Qwen の CPU 認識に対応します。CPU と Vulkan のどちらのビルドにも、GPU SDK やシェーダーコンパイラーは不要です。

Vulkan アクセラレーションを有効にする場合：

```powershell
npm run dev:vulkan
```

デスクトップの Vulkan 開発コマンドとリリースコマンドは、Vulkan ローダーを自動準備します。アプリは認識設定から Qwen ランタイムとモデルを必要に応じてダウンロードします。

スタンドアロンの Rust Core のみを実行する場合：

```powershell
npm run dev:core

# Optional Vulkan acceleration:
& .\scripts\prepare-vulkan-runtime.ps1
cargo run --manifest-path core/Cargo.toml --features vulkan
```

スタンドアロン Core の開発で Qwen ランタイムを手動準備する場合は、起動前に `& .\scripts\prepare-qwen-runtime.ps1` を実行してください。モデルパッケージは認識設定または Qwen モデル API からインストールできます。

CPU のみを使用する Core ビルド：

```powershell
cargo run --manifest-path core/Cargo.toml --no-default-features
```

これはデフォルトの Core ビルドと同じです。Qwen の自動デバイスモードは CPU を使用し、GPU を明示的に選択すると Vulkan バックエンドを利用できないことを報告します。`--features vulkan` を有効にすると、自動デバイスモードは互換性のある Vulkan GPU を優先し、GPU の起動に失敗した場合は CPU にフォールバックできます。

スタンドアロン Core はデフォルトで `http://127.0.0.1:8766` をリッスンし、字幕 WebSocket は `ws://127.0.0.1:8766/ws` で利用できます。デスクトップアプリはローカルセッショントークンを自動的に生成して管理します。Core を単独で実行し、ループバック以外のアドレスでリッスンする場合は、空でない `VRCS_SESSION_TOKEN` を明示的に設定する必要があります。

## テスト

```powershell
npm run check:i18n
npm --workspace apps/desktop test
npm run build:frontend
.\scripts\check-rust.ps1
```

Rust チェックスクリプトは Windows 上で両方の Rust crate のフォーマットチェック、Clippy（`-D warnings`）、テストを実行します。Vulkan 関連の変更には `.\scripts\check-rust.ps1 -Vulkan` も実行してください。Vulkan ローダーを準備し、Vulkan を有効にした両方の crate を検証します。Vulkan SDK は不要です。

## Release のビルド

リリーススクリプトは CPU と Vulkan に対応する標準インストーラーを1つだけビルドし、Vulkan ローダーを自動準備します。更新用の署名、SHA-256 チェックサム、`latest.json` も生成します。標準版と旧 CUDA 版の更新先は、同じ標準インストーラーと署名を使用します。

Release ビルドには `TAURI_SIGNING_PRIVATE_KEY` と `TAURI_UPDATER_PUBLIC_KEY` が必要です。秘密鍵を暗号化している場合は `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` も設定します。秘密鍵はリポジトリに保存せず、安全にバックアップしてください。

標準の Windows インストーラーをビルドします。

```powershell
npm run build
```

## コントリビュート

コントリビュートする前に [CONTRIBUTING.md](CONTRIBUTING.md) をお読みください。インターフェース言語を追加または更新する場合は、[LOCALIZATION.md](LOCALIZATION.md) を参照してください。提出前に変更内容に関連するテストを実行し、生成されたビルド成果物はコミットしないでください。

## ライセンス

VRCS は [GNU Affero General Public License v3.0](LICENSE)（`AGPL-3.0-only`）の下で提供されています。サードパーティ製コンポーネントとそのライセンスについては、[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) を参照してください。
