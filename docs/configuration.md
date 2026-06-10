# 設定

設定は環境変数で行い、一部を CLI フラグで上書きできる(CLI が優先)。

## CLI フラグ

```
agentic-search <QUESTION> [OPTIONS]

  --provider <ollama|claude|openai>  LLM プロバイダ(既定: ollama)
  --model <NAME>                     モデル名の上書き
  --max-iterations <N>               収集→評価の最大反復回数(既定: 4)
  --output <PATH>                    レポートをファイルに書き出す
  -v, --verbose                      詳細ログ(stderr)
```

## 環境変数

| 変数 | 既定値 | 説明 |
|---|---|---|
| `AGS_LLM_PROVIDER` | `ollama` | `ollama` / `claude` / `openai` |
| `AGS_LLM_MODEL` | プロバイダ依存(下表) | 使用モデル |
| `AGS_LLM_BASE_URL` | プロバイダ依存(下表) | API ベース URL。OpenAI 互換サーバーにも向けられる |
| `ANTHROPIC_API_KEY` | – | claude 使用時に必須 |
| `OPENAI_API_KEY` | – | openai 使用時に必須 |
| `AGS_SEARCH_PROVIDER` | `duckduckgo` | `duckduckgo` / `searxng` |
| `AGS_SEARXNG_URL` | `http://localhost:8080` | searxng 使用時のベース URL |
| `RUST_LOG` | – | tracing フィルタの上書き(例: `agentic_search=debug`) |

### プロバイダ別の既定値

| プロバイダ | 既定モデル | 既定ベース URL | 認証 |
|---|---|---|---|
| ollama | `llama3.2:3b` | `http://localhost:11434` | 不要 |
| claude | `claude-sonnet-4-6` | `https://api.anthropic.com` | `ANTHROPIC_API_KEY` |
| openai | `gpt-4o-mini` | `https://api.openai.com` | `OPENAI_API_KEY` |

API キーは環境変数からのみ読み込む。設定ファイル・コード・ログには絶対に書かない([security.md](security.md) 参照)。

## 動作リミット(`config.rs` の `Limits`)

エージェントの自律性はコスト・実行時間・メモリの観点で必ず上限に縛られる。

| 項目 | 既定値 | 意味 |
|---|---|---|
| `max_iterations` | 4 | 収集→評価ループの最大回数 |
| `max_queries_per_iteration` | 4 | 1反復で実行する検索クエリ数 |
| `max_results_per_query` | 8 | 1クエリで取得する検索結果数 |
| `max_pages_per_query` | 3 | 1クエリで実際に本文取得するページ数 |
| `max_content_chars` | 6,000 | LLM に渡すページ本文の最大文字数 |
| `fetch_timeout_secs` | 20 | ページ取得タイムアウト |
| `max_response_bytes` | 2 MiB | レスポンス本文の読み込み上限 |

変更する場合は小型ローカルモデルのコンテキスト長(`llama3.2:3b` は実質 ~8K トークン運用)を考慮すること。
