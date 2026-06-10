# 開発ガイド

## 前提

- Rust(stable)
- 既定の LLM はローカルの [Ollama](https://ollama.com/)。API 課金なしで何度でも動作確認できる。

```sh
ollama serve              # サーバー起動(アプリ版なら自動起動)
ollama pull llama3.2:3b   # 既定モデルの取得(約2GB)
```

> Homebrew の formula 版(`brew install ollama`)は `llama-server` バイナリを同梱しないことがある。
> その場合は `brew install --cask ollama-app` でアプリ版を入れ、同梱の
> `/Applications/Ollama.app/Contents/Resources/ollama` を使うこと。

## コマンド

ワークスペース構成(`crates/core` = エンジン、`crates/cli` = CLI、`crates/gui` = GUI)。

```sh
cargo build --workspace                  # 全クレートのビルド
cargo fmt                                # フォーマット(コミット前必須)
cargo clippy --workspace --all-targets -- -D warnings  # リント(警告ゼロを維持)
cargo test --workspace                   # 全テスト
cargo test -p agentic-search-core fetch::guard   # モジュール単位のテスト
cargo test deduplicates_equivalent       # 単一テスト(名前の部分一致)
```

## 実行例

```sh
# CLI
cargo run --release -p agentic-search-cli -- "調査したい質問" \
  --max-iterations 2 \
  --output report.md \
  -v

# GUI(macOS)
cargo run --release -p agentic-search-gui
```

クラウドモデルを使う場合(実装済み、要 API キー):

```sh
ANTHROPIC_API_KEY=... cargo run -p agentic-search-cli -- "質問" --provider claude
OPENAI_API_KEY=...    cargo run -p agentic-search-cli -- "質問" --provider openai
```

設定の全体は [configuration.md](configuration.md)、画面操作は [user-guide.md](user-guide.md) を参照。

## GUI(gpui)に関する注意

- `gpui` は `runtime_shaders` feature を有効にしている。Metal シェーダをビルド時でなく起動時にコンパイルするため、**フル Xcode なし(Command Line Tools のみ)でビルドできる**。この feature を外すと `xcrun: unable to find utility "metal"` で失敗する。
- ウィジェットは `gpui-component`(Input / Button / TextView など)を使用。テキスト入力や Markdown 表示を自前実装しないこと。
- GUI から調査を実行するときは `runner::start` 経由で専用スレッド + tokio ランタイムに載せる。gpui の executor 上で reqwest を直接 await しないこと。

## コーディング規約

- **1関数1責務**。処理を詰め込まず、`agent/gatherer.rs` のように「検索」「ページ処理」「抽出」を分ける。
- 外部依存(LLM・検索・HTTP 取得)は必ず trait 越しに使う。エージェントのロジックはモックでオフラインテストできる状態を保つ(`agent/mod.rs` のテスト参照)。
- プロンプト文言の変更は `crates/core/src/agent/prompts.rs` のみで行い、ロジックコードに埋め込まない。
- 1ページ・1クエリの失敗は `warn` ログでスキップし、調査全体を落とさない。
- LLM の JSON 出力は崩れる前提で扱う(`llm/json.rs` の寛容パース、`gatherer::parse_extraction` の項目単位サルベージ)。
- セキュリティ要件(SSRF ガード・サイズ上限・秘密情報の扱い)は [security.md](security.md) に従う。新しい取得経路を足すときは必ずガードを通すこと。

## テスト方針

- ネットワークに出るテストは書かない。HTML パーサや JSON 抽出は文字列フィクスチャで、エージェントループはモック trait でテストする。
- 新しいプロバイダを追加したら、リクエスト組み立てなど純粋関数部分を切り出して単体テストする。
