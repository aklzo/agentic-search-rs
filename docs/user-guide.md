# 操作説明書

agentic-search の使い方。CLI と GUI の2つのフロントエンドがあり、どちらも同じ調査エンジン(`crates/core`)を使う。

## 事前準備(共通)

既定の LLM はローカルの Ollama。一度だけ以下を実施する。

```sh
ollama serve              # サーバー起動(アプリ版 Ollama なら自動起動)
ollama pull llama3.2:3b   # 既定モデルの取得(約2GB)
```

Claude / OpenAI を使う場合は API キーを環境変数に設定する(詳細: [configuration.md](configuration.md))。

```sh
export ANTHROPIC_API_KEY=...   # Claude を使う場合
export OPENAI_API_KEY=...      # OpenAI を使う場合
```

---

## GUI アプリ(macOS)

### 起動

```sh
cargo run --release -p agentic-search-gui
```

> API キーは環境変数から読むため、Claude / OpenAI を使う場合はキーを設定した
> ターミナルから起動すること。

### 画面構成

```
┌────────────┬──────────────────────────────────┐
│ 履歴 (N)  Finderで開く │ [ 質問入力欄                      ] │
│ ┌────────┐ │ [LLM: Ollama] [- 反復 2 +]   [調査開始] │
│ │ 質問…    │ │ ┌─ 進捗ステータス(実行中に表示)─────┐ │
│ │ 日付 スコア 削除│ │ │ 検索中: …  取得: …  自己評価: …      │ │
│ └────────┘ │ └──────────────────────────┘ │
│  (過去レポート一覧) │ ┌─ レポート表示(Markdown)──────────┐ │
│            │ │  調査結果・引用・自己評価が表示される      │ │
│            │ └──────────────────────────┘ │
└────────────┴──────────────────────────────────┘
```

### 調査の実行

1. 上部の入力欄に調査したい質問を入力する(複数行可・日本語可)
2. 必要に応じて設定を変更する
   - **LLM ボタン**: クリックするたびに Ollama → Claude → OpenAI と切替
   - **モデルドロップダウン**: プロバイダ内のモデルを選択。**Ollama はローカルサーバーにインストール済みのモデル一覧を起動時に自動取得**して表示する(サーバー未起動時は既定リスト)。Claude は Sonnet 4.6 / Haiku 4.5 / Opus 4.8、OpenAI は gpt-4o-mini / gpt-5-mini / gpt-5 から選択。プロバイダを切り替えても同名モデルがあれば選択を維持する
   - **反復 - / +**: 収集→自己評価ループの最大回数(1〜8)。多いほど網羅的だが時間がかかる
3. **調査開始** を押す
4. 進捗がステータス欄に流れる(計画完了 → 検索中 → 取得 → 自己評価 → 必要なら追加調査)
5. 完了するとレポートが下部に表示され、**自動で履歴に保存**される

実行中はボタンがスピナー表示になる。1回の調査は反復2回・ローカルモデルで数分程度。

### レポートの見方

- レポートは **既定で日本語** で生成される(質問や情報源が英語でも日本語に翻訳して合成。`AGS_REPORT_LANGUAGE` で変更可能)
- 冒頭に短い結論、続いて詳細セクション、引用番号 `[n]` と出典 URL 一覧
- 表示は Markdown としてレンダリングされ、ウィンドウサイズに収まりスクロールバーで最後まで読める。**URL リンクはクリックすると既定のブラウザで開く**
- 末尾の **Self-assessment** はエージェント自身による品質採点(鮮度・正確性・網羅性、0〜100)と既知の限界。**100点でないレポートは未検証情報を含むので出典を確認すること**

### 実行トレース(監査ログ)

レポート右上の **「トレースを表示」** ボタンで、その調査の全過程をタイムスタンプ付きで確認できる(「レポートを表示」で戻る):

- 計画されたクエリ一覧 / 実行された各検索 / 取得した各ページと得られた件数
- 各反復の自己評価スコアと、**軸ごとの指摘事項**(何が不足・不審とされたか)
- 評価が不足と判定した際の **追加クエリ案**(なぜ追加検索が行われたかの根拠)

トレースはレポートと一緒に `<日時>.trace.jsonl`(JSON Lines)として保存されるため、`jq` などでの機械処理や外部の監査にも使える。

### 履歴の管理(左サイドバー)

- 各行に質問・保存日・スコア(鮮=鮮度 / 正=正確性 / 網=網羅性)を表示
- **行をクリック**: そのレポートを右側に表示(トレースも「トレースを表示」で閲覧可)
- **削除**: そのレポートとトレースを即座に削除(確認ダイアログなし・復元不可)
- **Finderで開く**: 保存フォルダ
  `~/Library/Application Support/agentic-search/reports/` を Finder で開く。
  各調査は `<日時>.md`(本文)+ `<日時>.json`(メタデータ)+ `<日時>.trace.jsonl`(実行トレース)の
  3ファイルで保存されており、他のツールでそのまま利用できる

---

## CLI

### 基本

```sh
cargo run --release -p agentic-search-cli -- "調査したい質問"
```

ビルド済みバイナリを直接使う場合は `target/release/agentic-search "質問"`。

### 主なオプション

```sh
agentic-search "質問" \
  --provider ollama|claude|openai \  # LLM 切替(既定: ollama)
  --model llama3.2:3b \              # モデル名上書き
  --max-iterations 2 \               # 反復回数(既定: 4)
  --output report.md \               # レポートをファイル出力(省略時は標準出力)
  -v                                 # 進捗の詳細ログ(stderr)
```

レポートは既定で日本語。他言語にする場合は `AGS_REPORT_LANGUAGE=English` のように環境変数で指定する。

### 使用例

```sh
# ローカルモデルで軽く調査して結果を保存
cargo run --release -p agentic-search-cli -- \
  "WebAssembly のコンポーネントモデルの現状" --max-iterations 2 --output wasm.md

# Claude で高品質なレポートを生成(モデルも指定可能)
ANTHROPIC_API_KEY=... cargo run --release -p agentic-search-cli -- \
  "質問" --provider claude --model claude-sonnet-4-6 --output report.md

# ローカルの別モデルで実行(例: gemma3:12b を pull 済みの場合)
cargo run --release -p agentic-search-cli -- "質問" --model gemma3:12b
```

レポートは標準出力(または `--output`)、進捗ログとスコアは stderr に出るため、パイプ処理しても混ざらない。

---

## トラブルシューティング

| 症状 | 対処 |
|---|---|
| `ollama returned HTTP 500` / 接続エラー | `ollama serve` が起動しているか、`ollama pull llama3.2:3b` 済みかを確認。Homebrew formula 版は壊れていることがあるのでアプリ版を使う([development.md](development.md)) |
| `provider Claude requires an API key` | `ANTHROPIC_API_KEY`(OpenAI は `OPENAI_API_KEY`)を設定したシェルから起動する |
| 検索結果が0件・調査が進まない | ネットワーク接続と、DuckDuckGo への到達性を確認。`AGS_SEARCH_PROVIDER=searxng`(自前 SearXNG)や `AGS_SEARCH_PROVIDER=serper` + `SERPER_API_KEY=...`(Google 検索・安定/高速)にも切替可能 |
| レポートの品質が低い・繰り返しが多い | 3B モデルの限界。`--model`(CLI)や反復回数を増やす、または Claude / OpenAI に切り替える |
| GUI のウィンドウが開かない | ターミナルにパニックが出ていないか確認。`cargo run -p agentic-search-gui` で再ビルドして起動 |
