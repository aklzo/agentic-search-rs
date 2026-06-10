# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Web を Agentic に検索し、収集した情報を鮮度・正確性・網羅性の3軸で自己評価して、不足分を追加検索するリサーチツール(Rust)。Cargo ワークスペース: `crates/core`(エンジン)+ `crates/cli`(CLI)+ `crates/gui`(gpui 製 macOS GUI)。LLM は既定でローカル Ollama(Claude / OpenAI 切替可)。

## コマンド

```sh
cargo build --workspace
cargo fmt                                              # コミット前必須
cargo clippy --workspace --all-targets -- -D warnings  # 警告ゼロを維持
cargo test --workspace                                 # 全テスト(ネットワーク不要)
cargo test <name>                                      # 単一テスト(名前の部分一致)
cargo run -p agentic-search-cli -- "質問" -v           # CLI 実行(要 ollama serve + llama3.2:3b)
cargo run -p agentic-search-gui                        # GUI 起動(macOS)
```

## ドキュメント(詳細は必ずこちらを参照)

- [docs/architecture.md](docs/architecture.md) — 設計全体。計画→収集→自己評価ループ、ワークスペース構成、GUI とエンジンの接続、拡張ポイント
- [docs/development.md](docs/development.md) — 開発手順、Ollama セットアップ、gpui の注意点、コーディング規約、テスト方針
- [docs/configuration.md](docs/configuration.md) — CLI フラグ、環境変数、動作リミット
- [docs/security.md](docs/security.md) — SSRF ガード等の不変条件と変更時チェックリスト
- [docs/user-guide.md](docs/user-guide.md) — 操作説明書(GUI / CLI の使い方)
- [docs/design-rationale.md](docs/design-rationale.md) — 設計思想。なぜこの設計・このクレートか、代替案比較、振り返り
- [docs/agentic-architecture.md](docs/agentic-architecture.md) — 設計の根拠としたエージェントアーキテクチャ調査
- [learn/cli-implementation.md](learn/cli-implementation.md) — 実装ナレッジ(コア/CLI 編)。つまづき・クレートの罠・Rust 特有挙動の追体験用
- [learn/gui-implementation.md](learn/gui-implementation.md) — 実装ナレッジ(GUI/gpui 編)。同上

`docs/` は仕様・運用のリファレンス、`learn/` は実装経験の追体験用ナレッジという役割分担。実装でつまづいた知見は `learn/` に追記すること。

## 重要な不変条件

- フロントエンド(CLI / GUI)は core の公開 API のみ使用し、エンジンのロジックを複製しない
- 外部依存(LLM・検索・取得)は trait(`LlmClient` / `SearchProvider` / `PageFetcher`)越しに使い、エージェントロジックはモックでテスト可能に保つ
- プロンプト文言は `crates/core/src/agent/prompts.rs` にのみ置く
- 新しい外部アクセスは必ず SSRF ガードとリソース上限を通す(docs/security.md)
- GUI: gpui の `runtime_shaders` feature を外さない(Xcode なしでビルドするため)。調査実行は `runner.rs` の専用スレッド経由で行う
