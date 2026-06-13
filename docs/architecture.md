# アーキテクチャ

本ツールは「Web を検索して情報を収集し、**自分で品質を判定して足りない情報を取りに行く**」リサーチエージェントである。設計は [agentic-architecture.md](agentic-architecture.md) の調査に基づき、2026年時点のベストプラクティスである「強い単一ループ+自己評価」を採用した(同ドキュメント §5 の結論)。

## 採用パターンと根拠

| パターン | 参照 | 本ツールでの実装 |
|---|---|---|
| Plan-and-Execute | §1.2 | `planner` が質問をサブ質問と検索クエリに分解してから実行する |
| ReAct 型ループ | §1.1 | 検索 → 取得 → 抽出 → 観測を反復する `Gatherer` |
| Reflection / Evaluator-Optimizer | §1.3 | `evaluator` が収集結果を批評し、不足分の追加クエリを生成する |

マルチエージェント化(§2)は見送った。MAST(§4.4)が示す通り協調の失敗リスクが高く、単一プロセスの調査タスクではコンテキスト分離の利益が小さいため。

## 処理フロー

```
質問
 │
 ▼
Planner ──────────── 質問を sub_questions + 検索クエリ群に分解(LLM, JSON)
 │
 ▼
┌─ 反復ループ(max_iterations まで)─────────────────────┐
│ Gatherer: クエリごとに                                  │
│   SearchProvider.search() → 未訪問 URL を選別            │
│   PageFetcher.fetch()     → SSRF ガード+本文抽出        │
│   LLM 抽出               → 出典・日付付き finding 化     │
│   KnowledgeStore         → 正規化ハッシュで重複排除(新規性判定)│
│                                                         │
│ Evaluator: findings ダイジェストを LLM が3軸で採点        │
│   freshness   … 今日の日付に対して情報が新しいか          │
│   correctness … finding 間の矛盾・単一ソースの怪しさ      │
│   coverage    … 質問の全側面に答えているか                │
│   → is_sufficient かつ全軸 70 点以上なら脱出              │
│   → 不足なら followup_queries を次の反復のクエリに         │
└─────────────────────────────────────────┘
 │
 ▼
Reporter ─── findings から引用付き Markdown レポートを合成し、
             自己評価スコアと既知の限界を末尾に明記する
```

### 終了条件(暴走防止)

1. 評価が `sufficient()`(LLM の判定 + 全軸スコア閾値の二重チェック)
2. `max_iterations` 到達
3. 追加クエリも新規 finding もない(進捗なしの早期終了)

実行済みクエリ・訪問済み URL は `KnowledgeStore` が記録し、同じ作業を繰り返さない。

### 収集フェーズの並列実行とリトライ

1クエリ内の処理は3段に分かれる(`gatherer.rs`):

1. **選択(逐次)**: 検索ヒットから未訪問ページを `max_pages_per_query` 件選び、その場で訪問済みにする。訪問管理とページ上限を決定的に保つため逐次
2. **取得+抽出(並列)**: 選んだページの「取得 → LLM 抽出」を `max_concurrent_pages` 本まで同時実行(`futures::buffered`、入力順を保持)。各ステージは `KnowledgeStore` に触れない純粋関数(`extract_page`)なのでロック不要
3. **マージ(逐次)**: 結果を順に `add_finding` で取り込む。重複排除は「先勝ち」なので逐次マージで再現性を保つ

並列度はプロバイダ別の既定(ローカル=1 / API=4)。ローカル LLM は単一 GPU が1リクエストの prefill で飽和するため並列が効かず、既定1で従来どおり逐次動作する。**クエリ間は逐次のまま**(検索 API のレート制限・DuckDuckGo の 429 回避)。

取得・LLM 呼び出しは一時障害(タイムアウト・5xx・429)に対し指数バックオフで `max_retries` 回まで再試行する(`retry.rs`)。並列化でバースト的なアクセスになり一時エラー率が上がるため、並列化とリトライはワンセット。再試行対象の判定は `AgentError::is_retryable`(4xx・SSRF 拒否・パース失敗は再試行しない)。

## ワークスペース構成

調査エンジン(core)とフロントエンド(CLI / GUI)を分離した Cargo ワークスペース。フロントエンドはどちらも core の公開 API(`Config` → ファクトリ → `ResearchAgent`)だけを使う。

```
crates/
  core/          ライブラリ agentic-search-core(エンジン本体)
    src/
      lib.rs       公開モジュールの宣言
      config.rs    環境変数 + 上書きの設定。SecretKey は Debug 出力でマスク
      error.rs     thiserror による統一エラー型(`is_retryable` で一時障害を分類)
      retry.rs     指数バックオフ再試行(取得・LLM 呼び出しが利用)
      events.rs    AgentEvent(フロントエンド向け進捗イベント)と EventSink
      llm/         LlmClient trait と各プロバイダ実装
        ollama.rs  既定。ローカル実行でコストゼロ
        claude.rs  Anthropic Messages API
        openai.rs  OpenAI 互換 Chat Completions
        json.rs    LLM 出力からの寛容な JSON 抽出
      search/      SearchProvider trait(duckduckgo / searxng)
      fetch/       PageFetcher trait、SSRF ガード、HTML→テキスト抽出(Readability + フォールバック)
      agent/       エージェント本体
        mod.rs       ResearchAgent(反復ループの制御・イベント発行)
        planner.rs   計画
        gatherer.rs  収集(1クエリの実行)
        evaluator.rs 自己評価
        reporter.rs  レポート合成
        knowledge.rs KnowledgeStore(重複排除・訪問管理・ダイジェスト)
        prompts.rs   全プロンプトを集約(挙動調整はここだけ触る)
  cli/           バイナリ agentic-search(従来どおりの CLI)
    src/main.rs    設定構築と配線、レポートの stdout/ファイル出力
    src/cli.rs     clap による CLI 定義
  gui/           バイナリ agentic-search-gui(gpui 製 macOS アプリ)
    src/main.rs    gpui Application 起動・ウィンドウ生成
    src/app.rs     メインビュー(入力・実行・進捗・レポート表示・履歴)
    src/runner.rs  専用スレッド + tokio ランタイムで調査を実行し
                   チャネルで進捗(RunUpdate)を UI に中継
    src/history.rs レポート保存ストア(md + json メタデータ、一覧・削除)
```

### GUI とエンジンの接続

GUI スレッド(gpui)と調査(tokio / reqwest)は実行モデルが異なるため、`runner.rs` が専用スレッドに tokio ランタイムを立てて実行する。core の `ResearchAgent::with_events` に登録したコールバックが `events::AgentEvent` を unbounded channel に流し、GUI 側は `cx.spawn` で受信してステータス表示を更新する。完了レポートは `history::HistoryStore` が `~/Library/Application Support/agentic-search/reports/` に保存し、サイドバーの履歴(閲覧・削除)に反映される。

### 実行トレース(監査ログ)

`AgentEvent` は serde でシリアライズ可能で、`EvaluationDone` は評価の全文(各軸のスコア・指摘事項・追加クエリ案)を運ぶ。GUI は受信した全イベントを `events::TraceRecord`(タイムスタンプ付き)として蓄積し、レポート保存時に `<日時>.trace.jsonl`(JSON Lines)として併置する。これにより「どのクエリを実行し、何を取得し、なぜ追加調査を行ったか」を実行後に追跡できる。トレースの読み書き(`to_jsonl` / `from_jsonl`)は core にあり、CLI など他フロントエンドからも再利用できる。

## 拡張ポイント

3つの trait がすべての外部依存を抽象化しており、テストではモックに差し替えている(`agent/mod.rs` の統合テスト参照)。

- **LLM プロバイダ追加**: `llm::LlmClient` を実装し、`config.rs` の `LlmProviderKind` と `llm::build_client` に1分岐追加する。
- **検索エンジン追加**: `search::SearchProvider` を実装し、`SearchProviderKind` と `build_provider` に追加する。
- **取得方法の差し替え**(ヘッドレスブラウザ等): `fetch::PageFetcher` を実装する。

LLM への入力サイズは `Limits.max_content_chars`(ページ本文)と `DIGEST_BUDGET`(findings ダイジェスト)で制御しており、小型ローカルモデルのコンテキスト長に合わせて調整できる(コンテキスト管理の考え方は agentic-architecture.md §0-3, §4.1 を参照)。
