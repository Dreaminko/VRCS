<p align="center">
    <img src="apps/desktop/public/logos/VRCS_Logo.svg" width="50%" alt="logo" />
</p>

# VRCS

[English](README.md) | [简体中文](README.zh-CN.md) | **日本語**

![](./screenshots/01.png)

VRCS は、VRChat 向けの Windows リアルタイム字幕・言語学習ツールです。システム出力、VRChat プロセスの音声、マイク入力を取り込み、デスクトップまたは SteamVR に字幕を表示します。さらに、その字幕を翻訳、辞書検索、学習分析、Anki カード作成、VRChat Chatbox への出力に活用できます。

[最新版をダウンロード](https://github.com/Dreaminko/VRCS/releases/latest) · [問題を報告](https://github.com/Dreaminko/VRCS/issues) · [コントリビュート](CONTRIBUTING.md)

[Discord](https://discord.gg/53H872eYq) · [QQ グループ](https://qm.qq.com/q/i9kOOxFn44)

## ダウンロードとインストール

[GitHub Releases](https://github.com/Dreaminko/VRCS/releases) から用途に合ったインストーラーをダウンロードしてください。

| インストーラー | 用途 | 追加要件 |
|---|---|---|
| `VRCS-<version>-windows-x64.exe` | ほとんどのユーザーに推奨。クラウド認識とローカル CPU 版 Whisper に対応 | CUDA は不要 |
| `VRCS-<version>-windows-x64-CUDA.exe` | NVIDIA GPU でローカル Whisper を高速化 | CUDA 12.x Runtime、cuBLAS、互換性のある NVIDIA GPU とドライバー |

標準版と CUDA 版は同じ設定、データベース、モデルディレクトリを共有するため、データを移行せずに切り替えられます。

動作要件：

- [Microsoft Visual C++ v14 Redistributable（x64）](https://aka.ms/vs/17/release/vc_redist.x64.exe)
- 初回起動時に、固定バージョンの Silero VAD モデルをダウンロードするためのインターネット接続
- ローカル Whisper を使用する場合は、選択したモデルの初回ダウンロード
- クラウド認識、翻訳、学習分析を使用する場合は、選択したサービスプロバイダーの API 認証情報。プロバイダーによって料金が発生する場合があります

## はじめに
初回起動時にセットアップウィザードが開きます。

1. 簡体字中国語、日本語、英語、またはシステム言語を選択します。
2. クラウドのリアルタイム認識またはローカル Whisper を選択します。
3. システム音声、VRChat プロセス音声、マイクを設定します。
4. マイクをテストし、音声トリガーのしきい値を調整します。
5. セットアップを完了し、文字起こしを開始します。

設定はアプリ内でいつでも変更できます。セットアップウィザードは「設定 → システム」から再実行できます。

## 主な機能

### リアルタイム字幕と音声

- Windows WASAPI システムループバック、VRChat プロセス専用ループバック、マイクキャプチャ
- システム音声とマイクの2系統文字起こし。音声ソースとデバイスを個別に制御可能
- Silero ONNX VAD。モデルを利用できない場合はエネルギーベース検出へ自動的にフォールバック
- リアルタイムの逐次字幕、確定字幕、セッション履歴、コンパクトウィンドウモード
- 字幕履歴をローカル SQLite に保存し、セッション単位で整理、名前変更、削除が可能

### 音声認識

- ローカル `whisper.cpp`。CPU とオプションの CUDA アクセラレーションに対応
- ローカル Whisper モデルのダウンロード、整合性検証、移行、削除
- Alibaba Cloud Qwen3 ASR と Fun-ASR によるリアルタイムストリーミング認識
- OpenAI Realtime Transcription
- クラウドサービスの自動再接続と設定可能な障害処理ポリシー

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
- Overlay には原文、翻訳、または両方を表示可能。システム音声、マイク、Chatbox の各ソースを選択し、位置、サイズ、透明度、表示時間を調整可能

### 辞書検索、学習、Anki

- Yomitan 辞書パッケージのインポートと管理
- 字幕内の単語を選択して検索し、元の文と翻訳のコンテキストを保持
- リアルタイム字幕、字幕履歴、検索結果から学習素材を収集
- 選択した LLM を使用した文脈に応じた語義説明、文型分析、会話レビュー
- 単語カード、文型カード、穴埋めカードの下書きを編集
- デッキ、ノートタイプ、フィールドマッピングを選択し、AnkiConnect 経由でカードを作成

## プライバシーとデータ

VRCS は元の音声を保存しません。字幕履歴、セッション、学習項目、辞書、設定はデフォルトでローカルに保存されます。

ローカル Whisper を使用する場合、音声はクラウドに送信されません。クラウド認識を使用する場合、検出された音声区間が選択した認識サービスプロバイダーに送信されます。クラウド翻訳または学習分析を使用する場合、関連テキストとユーザーが明示的に選択したコンテキストが対応するプロバイダーに送信されます。

## ソースから実行

開発環境：

- Windows 10 / 11
- Node.js 24+
- Rust stable
- Visual Studio Build Tools と「C++ によるデスクトップ開発」ワークロード
- CUDA 開発の場合のみ、NVIDIA CUDA Toolkit と設定済みの `CUDA_PATH`

```powershell
npm install
npm run dev
```

デフォルトのコマンドは CPU ビルドを使用します。CUDA を有効にする場合：

```powershell
npm run dev:cuda
```

スタンドアロンの Rust Core のみを実行する場合：

```powershell
npm run dev:core
npm run dev:core:cuda
```

スタンドアロン Core はデフォルトで `http://127.0.0.1:8766` をリッスンし、字幕 WebSocket は `ws://127.0.0.1:8766/ws` で利用できます。デスクトップアプリはローカルセッショントークンを自動的に生成して管理します。Core を単独で実行し、ループバック以外のアドレスでリッスンする場合は、空でない `VRCS_SESSION_TOKEN` を明示的に設定する必要があります。

## テスト

```powershell
npm run check:i18n
npm --workspace apps/desktop test
npm run build:frontend
.\scripts\test-core.ps1
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
```

## Release のビルド

Release ビルドには `TAURI_SIGNING_PRIVATE_KEY` と `TAURI_UPDATER_PUBLIC_KEY` が必要です。秘密鍵を暗号化している場合は `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` も設定します。秘密鍵はリポジトリに保存せず、安全にバックアップしてください。

標準の Windows インストーラーをビルドします。

```powershell
npm run build
```

標準版と CUDA 版を両方ビルドします。

```powershell
.\scripts\build-release.ps1 -Version 0.1.0 -IncludeCuda
```

## コントリビュート

コントリビュートする前に [CONTRIBUTING.md](CONTRIBUTING.md) をお読みください。インターフェース言語を追加または更新する場合は、[LOCALIZATION.md](LOCALIZATION.md) を参照してください。提出前に変更内容に関連するテストを実行し、生成されたビルド成果物はコミットしないでください。

## ライセンス

VRCS は [GNU Affero General Public License v3.0](LICENSE)（`AGPL-3.0-only`）の下で提供されています。サードパーティ製コンポーネントとそのライセンスについては、[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) を参照してください。
