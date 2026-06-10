# agentic-search

Web を Agentic に検索して情報を網羅的に収集する Rust 製ツール。CLI と macOS GUI(gpui)の2つのフロントエンドを持つ。

質問を与えると、エージェントが検索クエリを計画し、Web ページを収集・抽出したうえで、**鮮度(新しいか)・正確性(矛盾がないか)・網羅性(全側面に答えているか)** を自己評価し、不足があれば追加の検索を自律的に行う。最終成果物は出典付きの Markdown レポート。

## 特徴

- **自己評価ループ**: LLM が収集結果を3軸で採点し、不足分の検索クエリを自分で生成して再調査する
- **ローカル LLM 既定**: Ollama で API コストゼロで動作。Claude / OpenAI にもワンフラグで切替可能
- **キー不要で動く検索**: DuckDuckGo(セットアップ不要)。SearXNG にも切替可能
- **セキュア設計**: SSRF ガード(プライベート IP・リダイレクト再検証)、レスポンスサイズ上限、API キーのログマスク
- **GUI 付き**: 進捗のリアルタイム表示、レポートの Markdown 閲覧、履歴の保存・管理

## 必要環境

| 対象 | 要件 |
|---|---|
| 共通 | Rust(stable)、[Ollama](https://ollama.com/) + モデル(既定: `llama3.2:3b`) |
| CLI | macOS / Linux / Windows(rustls 使用のため OpenSSL 不要) |
| GUI | macOS(Command Line Tools のみで可。フル Xcode 不要) |
| クラウド LLM 利用時 | `ANTHROPIC_API_KEY` または `OPENAI_API_KEY` |

## クイックスタート

```sh
# ローカル LLM(既定・無料)
ollama serve
ollama pull llama3.2:3b

# GUI(macOS): 入力・実行・進捗表示・レポート閲覧・履歴管理
cargo run --release -p agentic-search-gui

# CLI
cargo run --release -p agentic-search-cli -- "調査したい質問" --output report.md
```

Claude / OpenAI を使う場合は `--provider claude` / `--provider openai`(GUI では LLM ボタンで切替。API キー必須)。

## 構成

| クレート | 内容 |
|---|---|
| `crates/core` | 調査エンジン(計画 → 収集 → 自己評価ループ) |
| `crates/cli` | CLI フロントエンド(バイナリ名 `agentic-search`) |
| `crates/gui` | gpui 製 macOS GUI(進捗表示・レポート閲覧・履歴管理) |

## ドキュメント

| ドキュメント | 内容 |
|---|---|
| [docs/user-guide.md](docs/user-guide.md) | **操作説明書**(GUI / CLI の使い方) |
| [docs/architecture.md](docs/architecture.md) | 設計(エージェントループ・ワークスペース構成) |
| [docs/configuration.md](docs/configuration.md) | CLI フラグ・環境変数・動作リミット |
| [docs/development.md](docs/development.md) | ビルド・テスト・コーディング規約・gpui の注意点 |
| [docs/security.md](docs/security.md) | SSRF 対策などのセキュリティ設計 |
| [docs/design-rationale.md](docs/design-rationale.md) | 設計思想(なぜこの設計・このクレートか、代替案比較) |
| [docs/agentic-architecture.md](docs/agentic-architecture.md) | 設計根拠としたアーキテクチャ調査 |
| [learn/cli-implementation.md](learn/cli-implementation.md) | 実装ナレッジ コア/CLI 編(つまづき・クレートの罠・Rust 特有挙動) |
| [learn/gui-implementation.md](learn/gui-implementation.md) | 実装ナレッジ GUI/gpui 編(ビルドの罠・gpui のメンタルモデル) |

`docs/` は仕様・運用リファレンス、`learn/` は実装を追体験するためのナレッジ集。

## 開発

```sh
cargo fmt
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace   # ネットワーク・Ollama 不要で完走する
```

詳細は [docs/development.md](docs/development.md) を参照。

## ライセンス

[MIT](LICENSE)
