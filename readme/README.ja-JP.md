<div align="center">

<img src="logo.png" alt="e のロゴ" width="128">

# e

</div>

<div align="center">

[English](../README.md) | [简体中文](README.zh-CN.md) | 日本語 | [Français](README.fr-FR.md)

</div>

`e` はコーディングエージェント向けのターミナル UI です。`dshe` は [DeepSeek Harness (DSH)](https://github.com/deepseek-ai/deepseek-harness) に接続し、`pie` は公式 RPC インターフェース経由で Pi を実行します。どちらのエージェントランタイムも置き換えません。

> 開発初期のソフトウェアです。主なサポート環境は Windows と PowerShell で、Linux x64 向けのビルド済みバイナリも提供しています。

https://github.com/user-attachments/assets/45a75c55-7e3b-4af3-b1e2-1c6306b80358

## クイックスタート

### インストール

使うエージェントに合わせてフロントエンドを選んでください。以下のコマンドには [Node.js と npm](https://nodejs.org/)、[Rust と Cargo](https://rustup.rs/) が必要です。DSH には [Git](https://git-scm.com/) と pnpm も必要です。

**DeepSeek Harness — `dshe`**

```bash
npm install --global pnpm @deepseek-ai/dsh
cargo install e-dsh --locked
dshe setup
```

**Pi — `pie`**

```bash
npm install --global @earendil-works/pi-coding-agent
cargo install e-pi --locked
```

Rust をインストールしない場合は、[GitHub Releases](https://github.com/gloridifice/e/releases) から Windows x64 の ZIP または Linux x64 の tarball をダウンロードしてください。`SHA256SUMS` で検証し、実行ファイルを `PATH` の通ったディレクトリに展開します。各アーカイブには両方のフロントエンドが含まれます。選んだエージェントの npm 依存パッケージは上記のコマンドでインストールしてください。DSH には引き続き Git と `dshe setup` が必要です。

プロジェクトのディレクトリで `dshe` または `pie` を実行し、`/login` でプロバイダーへの接続を設定します。`dshe` は専用の DSH サービスを自動起動します。`pie` は Pi の認証情報、モデル、拡張機能、セッションファイルをそのまま利用します。

どちらのフロントエンドでも `/help` でキー操作を確認できます。Pi の起動オプションは `pie --help` を参照してください。互換性の詳細は [DSH のセットアップと更新](../doco/guides/dsh-integration.md)、[Pi ランタイムの要件](../doco/guides/development.md#prerequisites)に記載しています。

### 更新

選んだフロントエンドに対応する npm と Cargo のインストールコマンドを再実行してください。ビルド済みバイナリを使う場合は、最新リリースの実行ファイルに置き換えます。

DSH または `dshe` の更新後は `dshe setup` を再実行し、稼働中の DSH サービスを再起動して新しいブリッジを読み込んでください。`/reload` ではこの再起動を代替できません。

## 機能

- コードのシンタックスハイライト、表、Mermaid 図、diff プレビューに対応した Markdown のストリーミング表示。
- メッセージやツール出力を確認できる、幅を調整可能な Preview ペインと Reading モード。
- 複数行編集、入力履歴、コマンドとパスの補完、プロンプトのキュー。
- モデルと推論強度の選択、再利用可能なプロンプトマクロ、コンテキスト圧縮用モデルの設定。
- Pi ネイティブの認証、MCP 管理、セッションの再開・分岐・複製。
- モデルのコンテキストやエージェントの履歴に残らないローカル shell コマンド。非対話形式で、ユーザー自身の権限で実行されます。
- 操作時間と token／コストのタイムラインを確認できる実行履歴。エクスポートしたメタデータには、機密性のあるコマンド引数が含まれる場合があります。
- カスタムテーマ、変更可能なキーバインド、英語または簡体字中国語の UI。

## 設定

一般的な設定は `/settings` で変更できます。両フロントエンドで共有する設定ディレクトリは `/econfig` で確認できます。

- Windows：`%APPDATA%\e`
- Linux：`$XDG_CONFIG_HOME/e`（既定値は `~/.config/e`）
- macOS：`~/.config/e`

設定は `config.toml` に保存されます。簡体字中国語を使うには `language = "zh-CN"` を指定してください。既定の言語とフォールバック言語は英語です。エージェントの認証情報とセッションファイルは引き続き DSH または Pi が管理します。

`key_mapping.toml` の変更には[キーマッピングガイド](../doco/guides/key-mapping.md)と[既定のキーバインド](../crates/e-tui/assets/default_key_mapping.toml)を参照してください。カスタムテーマの TOML ファイルは `themes/` に配置します。[同梱テーマ](../crates/e-tui/assets/themes/)を参考にできます。`/reload` は設定、テーマ、バックエンドのリソースを再読み込みします。

## ソースからビルド

Git と Rust/Cargo が必要です。実行時には上記のランタイム要件も満たしてください。

```bash
git clone https://github.com/gloridifice/e.git
cd e
cargo build --release --locked -p e-dsh -p e-pi --bins
```

実行ファイルは `target/release/` に生成されます。チェックアウトしたソースからのインストール、ブリッジの変更、プロファイリング用ビルドについては[開発ガイド](../doco/guides/development.md)を参照してください。

## ライセンス

[MIT](../LICENSE-MIT) または [Apache-2.0](../LICENSE-APACHE) のいずれかを選択できます。

## 開発

Issue は歓迎します。プロトタイプ段階のため、Pull Request はまだ受け付けていません。

- [ドキュメント一覧](../doco/README.md)
- [開発ガイド](../doco/guides/development.md)
- [アーキテクチャ](../doco/architecture.md)
- [テストと検証](../doco/guides/testing.md)
- [トラブルシューティング](../doco/guides/troubleshooting.md)
