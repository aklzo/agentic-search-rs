# 実装ナレッジ: コアエンジン / CLI 編

実装中に実際につまづいた点・クレート固有の罠・他言語にない Rust 特有の挙動・見落としやすい標準設定を、追体験できる粒度で記録する。「どう動くか」は [../docs/architecture.md](../docs/architecture.md)、「なぜそうしたか」は [../docs/design-rationale.md](../docs/design-rationale.md) を参照。本書は **実装の現場で何が起きたか** に特化する。

---

## 1. 環境構築: Homebrew 版 Ollama は推論できない(実害あり)

最初の E2E 実行は Ollama の **HTTP 500** で失敗した。`ollama pull` は成功し `ollama list` にもモデルが出るのに、`/api/chat` を叩いた瞬間に落ちる。サーバーログの実物:

```
msg="failed to create server" model=llama3.2:3b
error="error starting llama-server: llama-server binary not found
(checked: /opt/homebrew/Cellar/ollama/0.30.7/libexec/lib/ollama/llama-server, ...)
Run 'cmake -S llama/server --preset cpu && cmake --build --preset cpu' first"
```

- **原因**: Homebrew formula 版(`brew install ollama`)0.30.7 は推論バックエンドの `llama-server` バイナリを同梱していない(`libexec/lib/ollama/` には `mlx_metal_v3` しか無い)。`brew reinstall` しても直らない。
- **解決**: アプリ版 `brew install --cask ollama-app` を入れ、同梱の `/Applications/Ollama.app/Contents/Resources/ollama` でサーバーを起動する(`llama-server` も同ディレクトリに存在する)。
- **教訓**: 「`ollama list` が通る」と「推論が動く」は別物。導通確認は必ず実際の chat リクエストで行う:

```sh
curl -s http://localhost:11434/api/chat \
  -d '{"model":"llama3.2:3b","stream":false,"messages":[{"role":"user","content":"say ok"}]}'
```

## 2. 小型ローカルモデル × JSON の現実

エージェントは全工程(計画・抽出・評価)で LLM に JSON を要求するが、llama3.2:3b は **Ollama の `format: "json"` を指定していても壊れた JSON を返す**。実際に観測した壊れ方:

```
{"findings": [{"statement": "..."}],":[{"        ← 末尾にゴミトークン
{"findings': [{ ...                              ← クォートの混在(" と ' )
{"findings": ["not an object", {...}]}           ← 配列要素の型が混ざる
```

`format: "json"` は「JSON らしいトークンに偏らせる」程度の保証しかなく、**スキーマはおろか構文の保証すらない** と考えるべき。プロバイダごとの実情:

| プロバイダ | JSON 強制 | 実装上の扱い |
|---|---|---|
| Ollama | `format: "json"`(構文も時々破る) | 指定した上で寛容パース必須 |
| OpenAI | `response_format: {"type":"json_object"}`(かなり堅い) | ネイティブ指定 |
| Claude | ネイティブ JSON モードなし | system プロンプト末尾に「JSON のみで返答」を注入 |

### 防御の階層化(これをしないと運用に耐えない)

1. **寛容な JSON 抽出**(`llm/json.rs`): まず全体を `from_str`、ダメなら最初の `{` / `[` から **文字列リテラル対応の括弧バランス走査** で JSON 領域を切り出す。正規表現では `"uses { and }"` のような文字列内括弧で破綻するため、`in_string` / `escaped` の状態機械で書く。
2. **項目単位サルベージ**(`gatherer::parse_extraction`): `findings` 配列の各要素を **個別に** `serde_json::from_value` し、失敗要素だけ捨てる。最初は配列全体を一括デシリアライズしていて、1要素の型崩れでページ全体を捨てていた(ログに `invalid type: string "findings", expected struct ExtractedFinding` が頻発して気づいた)。トップレベルが裸の配列で返るケースも吸収する。
3. **`#[serde(default)]` の全面適用**: 評価 JSON のキー欠落(小型モデルは平気でキーを落とす)で全体を失敗させない。`Evaluation` は全フィールド default 付きで、部分的な出力でも採点不能ではなく「0点扱い→追加調査」に倒れる。

## 3. Rust 特有の挙動(他言語経験者が踏む順)

### 3.1 文字列スライスはバイト位置(日本語で panic する)

`&text[..200]` は **バイト境界** で切るため、マルチバイト文字の途中だと panic する。Python の `s[:200]` の感覚で書くと日本語入力で即死する。本リポジトリでは3箇所(エラープレビュー、ダイジェスト、UI 表示)すべてで `char_indices().nth(n)` か `is_char_boundary()` による切り詰めを使っている。**「LLM やユーザー由来の文字列を固定長に切る」処理は全部これが必要**。

### 3.2 trait + async は素では書けない

`SearchProvider` のような **dyn で持ちたい trait** に `async fn` を生やすには `async-trait` クレートが必要(ネイティブの async-fn-in-trait は dyn 互換でない)。`#[async_trait]` を trait 定義と **全 impl の両方** に付け忘れるとエラーメッセージが分かりにくい。

### 3.3 dead_code 解析は Debug derive を「使用」とみなさない

ビルド時に出た実物の警告:

```
warning: field `sub_questions` is never read
  = note: `Plan` has a derived impl for the trait `Debug`, but this is
          intentionally ignored during dead code analysis
```

`{:?}` でログに出していてもフィールドは「未使用」扱い。`#[allow(dead_code)]` で黙らせるのではなく、実際にログ・表示で使うよう直した。**警告は設計の歪み(取ったのに使っていないデータ)を指していることが多い**。

### 3.4 エラー型は「ライブラリ=thiserror / バイナリ=anyhow」で分ける

core は `thiserror` の列挙型(呼び出し側が `BlockedUrl` と `Http` を区別できる)、CLI/GUI は `anyhow` + `.context()`。`#[from]` を付けると `?` が自動変換するので、reqwest/serde_json/io のエラーが境界で型に吸い込まれる。全部 anyhow にするとフロントエンドがエラー種別で分岐できなくなる。

### 3.5 edition は 2021 を明示

`cargo init` は edition 2024 を生成するが、GUI で使う gpui エコシステムとの互換を考えて 2021 に揃えた。edition はワークスペース全体で `workspace.package` から継承させる。

## 4. reqwest の標準設定は「楽観的」— 全部上書きした

reqwest はデフォルトのままだとセキュリティ・資源面で穴になる。**明示的に変えた設定**:

| デフォルト挙動 | 問題 | 対処 |
|---|---|---|
| リダイレクトを黙って10回追う | 公開 URL → `Location: http://169.254.169.254/` で SSRF ガードを素通り | `redirect::Policy::custom` で **各ホップを再検証**(`attempt.url()` を guard に通し、NG なら `attempt.error()`) |
| レスポンスサイズ無制限 | `.bytes()` は相手次第でメモリを食い尽くす。`Content-Length` は詐称可能 | `chunk()` でストリーム読みして 2MiB で打ち切り(ヘッダを信じない) |
| タイムアウトなし | ハングしたページで反復が止まる | `timeout`(全体)+ `connect_timeout` を別々に設定 |
| TLS は native-tls(OpenSSL) | システム依存・ビルド環境差 | `default-features = false` + `rustls-tls`(`json`, `gzip` だけ足す) |

`default-features = false` にすると **TLS が一切無くなる**(https が `error trying to connect` で謎エラーになる)ので、rustls-tls の付け忘れに注意。

## 5. SSRF ガードの細部(std だけでは書けない)

- `Ipv4Addr::is_global()` / `is_private()` 系のうち **`is_global()` は unstable**(`ip` feature)で stable では使えない。is_private は RFC1918 のみで CGNAT(100.64.0.0/10)や 0.0.0.0/8 を含まない。結局 **公開判定は自前実装** になる。
- **IPv4-mapped IPv6 が最大の抜け道**: `http://[::ffff:127.0.0.1]/` は IPv6 として見るとどの非公開レンジにも該当しない。`v6.to_ipv4_mapped()` で剥がして IPv4 判定に回す処理を忘れると loopback に到達できてしまう。テストに明示的に入れてある。
- ホスト名の DNS 検証(`tokio::net::lookup_host`)には tokio の **`net` feature が必要**。`rt-multi-thread,macros` だけだとコンパイルエラーになる。
- DNS 検証テストには `localtest.me`(127.0.0.1 を返す公開ドメイン)を使用。オフライン環境では解決失敗=同じく拒否、なのでテストはネットワーク有無どちらでも通る、という書き方にした。

## 6. scraper / ego-tree の罠

- `Html::parse_document(html).root_element().text()` は **script/style の中身も全部返す**。可視テキスト抽出には木を自前で走査して除外タグをスキップするしかない。
- 再帰関数で走査を書くと `ego_tree::NodeRef<scraper::Node>` という **間接依存クレートの型名** をシグネチャに書く必要があり、`ego_tree` を直接依存に足す羽目になる。回避策: `let mut stack = vec![*root];`(`ElementRef` は `Deref<Target = NodeRef>` かつ Copy)で **型推論に任せるスタック走査** にすると依存を増やさず書ける。
- `Selector::parse()` は `Result` を返すが、静的な CSS セレクタ文字列なら失敗しえないので `expect("static selector")` で良い(動的セレクタなら当然 `?`)。
- `Html` は `!Send` なので async fn 内で await をまたいで保持できない。パース処理は同期関数に切り出す(これはテスト容易性の面でも正解だった)。

## 7. DuckDuckGo HTML エンドポイントの仕様

- 検索 API キーなしで使えるのは `https://html.duckduckgo.com/html/?q=...`(JS なしの HTML 版)。
- 結果リンクは直接 URL ではなく `//duckduckgo.com/l/?uddg=<URL エンコード済み>&rut=...` という **リダイレクトラッパー**。`url::Url::query_pairs()` で `uddg` パラメータを取り出してデコードする。スキーム省略(`//` 開始)の URL が来る点にも注意(`https:` を前置してからパース)。
- セレクタは `div.result` / `a.result__a` / `.result__snippet`。HTML 構造は予告なく変わりうるので、パーサは純関数に切り出して **フィクスチャ HTML でテスト** しておく(構造変更時にテストが先に割れる)。

## 8. tracing の小さな罠

- `EnvFilter` のターゲット名は **クレート名のハイフンをアンダースコアにしたもの**(`agentic-search-core` → `agentic_search_core=debug`)。ハイフンのまま書くと黙って何もマッチしない。
- ログは `with_writer(std::io::stderr)` で stderr に出す。レポート本文を stdout に出す CLI では、これを忘れるとパイプ処理でログが混入する。

## 9. テスト設計で効いたこと

- 外部依存3つ(LLM・検索・取得)を trait にした結果、**エージェントの反復ループ全体**(計画→収集→評価不足→追加クエリ→評価充足→レポート)が `MockLlm` / `MockSearch` / `MockFetcher` だけでオフラインテストできる。Mock LLM は「system プロンプト内の役割文字列で分岐し、評価役は1回目不足/2回目充足を返す」状態機械にした。
- `Mutex<u32>` で呼び出し回数を数える素朴な方法で十分(モックライブラリ不要)。
- ネットワークテストは書かない方針だが、唯一 DNS 検証だけは「失敗しても拒否方向に倒れる」性質を利用して環境非依存にした(§5)。
