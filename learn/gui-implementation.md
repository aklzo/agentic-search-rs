# 実装ナレッジ: GUI(gpui)編

gpui は Zed エディタの UI フレームワークで、ドキュメントが薄く「ソースを読んで学ぶ」前提のエコシステム。実装で実際に踏んだ問題と、gpui 特有のメンタルモデルを、追体験できる粒度で記録する。構成・接続方法は [../docs/architecture.md](../docs/architecture.md)、選定理由は [../docs/design-rationale.md](../docs/design-rationale.md) を参照。

---

## 1. ビルドを通すまでが最初の関門

### 1.1 gpui は crates.io 版が存在する(2025年以降)

長らく `git = "https://github.com/zed-industries/zed"` 直依存しかなかったが、現在は **crates.io に `gpui` 0.2.x が公開**されている。ウィジェット集の `gpui-component`(0.5.x)、アイコン・フォント用の `gpui-component-assets` も crates.io にあり、git 依存なしで完結する。バージョン互換は「gpui-component 0.5 ↔ gpui 0.2」の組で取る。

### 1.2 Metal シェーダ問題: フル Xcode がないとビルドが落ちる

最初のビルドはこれで失敗した:

```
cargo::error=metal shader compilation failed:
xcrun: error: unable to find utility "metal", not a developer tool or in PATH
```

- **原因**: gpui の build.rs が `shaders.metal` をビルド時にコンパイルする。Metal コンパイラは **Command Line Tools には含まれず、フル Xcode が必要**(`xcode-select -p` が `/Library/Developer/CommandLineTools` を指す環境では確実に落ちる)。
- **解決**: gpui の **`runtime_shaders` feature** を有効にする。シェーダを起動時にコンパイルする方式に切り替わり、CLT のみでビルドできる。

```toml
gpui = { version = "0.2", features = ["runtime_shaders"] }
```

- **見つけ方が重要**: この feature は docs.rs では事実上見つからない。`~/.cargo/registry/src/*/gpui-0.2.2/` を **直接 grep** して `Cargo.toml` の feature 定義と build.rs の `#[cfg(feature = "runtime_shaders")]` 分岐を見つけた。**ドキュメントが薄いクレートは vendored ソースを一次資料にする** — このエコシステムでは常套手段。
- なお features は cargo 全体で加算的なので、gpui-component が依存する gpui にも自分の指定が効く(自クレート側で指定するだけでよい)。

### 1.3 API の正は同バージョンタグの examples

gpui-component の使い方は、GitHub の `longbridge/gpui-component` の **使用バージョンと同じタグ**(v0.5.1)の `examples/` と `crates/story/` を読むのが最短だった。main ブランチは API が先に進んでいるため、タグを合わせないと写経してもコンパイルが通らない。

## 2. gpui のメンタルモデル(Web/他 GUI との違い)

### 2.1 状態は Entity、描画は毎フレーム作り直し

- 状態は `Entity<T>` に持たせ、`cx.new(|cx| ...)` で生成、`entity.read(cx)` / `update` でアクセスする。React の state とオーナーシップモデルの折衷のような設計。
- ビューは `Render` trait を実装した Entity。`render()` は **呼ばれるたびに要素ツリーを丸ごと作り直す**(仮想 DOM 的)。状態を変えたら `cx.notify()` を呼ばないと再描画されない。「変更したのに画面が変わらない」はほぼ notify 忘れ。
- スタイリングは Tailwind 風のメソッドチェーン(`div().flex().gap_2().p_4().rounded_md()`)。条件付きスタイルは `FluentBuilder` の `.when(cond, |this| ...)`(`use gpui::prelude::FluentBuilder as _` が必要)。

### 2.2 「メソッドが無い」エラーの正体は trait 未 import

実際に出たエラーと原因:

| エラー | 原因 |
|---|---|
| `no method named 'xsmall' found for struct 'Button'` | `gpui_component::Sizable` trait を import していない |
| `no method named 'disabled' found for struct 'Button'` | 同 `Disableable` trait |
| `no method named 'font_semibold' found for 'Div'` | そもそも存在しない。`.font_weight(FontWeight::SEMIBOLD)` が正 |
| `cx.theme()` が無い | `ActiveTheme` trait(`gpui_component::ActiveTheme as _`) |

gpui-component は機能を extension trait に分割しているため、**ビルダーメソッドが見つからない時はまず trait import を疑う**。`use gpui_component::*` で全部入れてしまう例が多いのはこれが理由。

### 2.3 (window, cx) の引き回しと Context の種類

- コンテキストは複数種類ある: `App`(アプリ全体)/ `Context<T>`(Entity 更新中)/ `Window` / `AsyncApp`(async 内)。メソッドにより要求が違い、特に **`TextView::markdown(id, md, window, cx)` のように一見純粋な生成関数が `&mut Window` を要求する**(内部で `window.use_keyed_state` を使い状態をウィンドウに紐づけるため)。`render()` から子の描画ヘルパーに `window` を引き回す設計に最初からしておくと後で楽。
- イベントハンドラは `cx.listener(|this, event, window, cx| ...)` で作る。クロージャは `'static` 必須なので、**ループ内で各行のデータ(履歴の stem 等)を `clone()` してキャプチャ**する。同じ行にクリックと削除の2ハンドラを付けるなら clone も2つ要る。

### 2.4 ElementId と stateful 要素

`.on_click()` や `.overflow_y_scroll()`(スクロール位置の保持)は **`.id(...)` を付けた stateful 要素にしか生えない**。リスト行のような繰り返し要素には `("history", index)` のようなタプル ID が使える。「`on_click` が見つからない」エラーの一定数は `.id()` 忘れ。

### 2.5 Root と init の儀式

ウィンドウのルートは自分のビューを直接置くのではなく `Root::new(view, window, cx)` で包む(通知・ダイアログ等のオーバーレイ層)。また `gpui_component::init(cx)` を **どのコンポーネントより先に** 呼ぶ。忘れた場合の挙動は panic ではなくテーマ未初期化の異常描画なので気づきにくい。

### 2.6 Subscription は変数に保持しないと無効になる

`cx.subscribe_in(...)` の戻り値 `Subscription` は **drop された時点で購読解除** される。公式例にも `_subscriptions: Vec<Subscription>` をフィールドに持つパターンが明記されている。`let _ = cx.subscribe_in(...)` と書くと「イベントが一度も来ない」という症状になる(エラーは出ない)。

## 3. ウィジェットは自作しない(特にテキスト入力)

**gpui 本体には TextInput が存在しない。** Zed 本体は自前エディタを持っているため、フレームワークとしては `EntityInputHandler` という低レベル trait(IME 変換中テキスト、選択範囲、カーソル矩形の管理など十数メソッド)を公開しているだけ。これを自前実装すると数百行かつ **日本語 IME 対応が地獄** になる。

`gpui-component` の `InputState` + `Input` を使えば IME・複数行(`.multi_line(true)`)・プレースホルダ込みで動く。日本語入力が要件にある時点で gpui-component はほぼ必須と考えてよい。値の取得は `self.input_state.read(cx).value()`。

Markdown 表示も同様に `TextView::markdown(...)`(`.selectable(true)` でテキスト選択可)があり、レポート表示はこれで済んだ。

### 3.1 TextView の2つのサイズモードとスクロール

長文表示で最初に間違えたのは「外側の `div().id().overflow_y_scroll()` でスクロールさせ、TextView は素のまま」という構成。TextView にはモードが2つあり、ソースの doc コメントに仕様が書いてある:

- **`scrollable(false)`(既定)**: コンテンツ全体に展開する。短いラベル向き。長文では親をはみ出す
- **`scrollable(true)`**: `gpui::list` による**仮想化レンダリング+スクロールバー表示**。ただし**親が確定した高さを持つことが必須**

長文レポートの正解は後者で、親は `div().flex_1().min_h(px(0.)).overflow_hidden()`(flexbox で確定高さを与える)にする。`min_h(0)` を忘れると flex アイテムの暗黙の `min-height: auto` で親がコンテンツに引き伸ばされ、ウィンドウからはみ出す — これは Web の flexbox と同じ罠。

### 3.2 リンククリックは組み込み済み

「Markdown のリンクをクリックで既定ブラウザで開く」は自前実装不要だった。gpui-component の inline レンダラが内部で `cx.open_url(&link.url)` を呼んでいる(`src/text/inline.rs`)。`cx.open_url` は gpui 標準 API で macOS では `open` 相当。**「実装が要りそうな機能は、まず vendored ソースを grep して既存実装を探す」**がここでも効いた。

## 4. 非同期の二重世界: gpui executor と tokio は別物

ここが GUI 実装最大の設計ポイント。

- gpui は **独自の foreground/background executor** を持つ。一方 reqwest / `tokio::net::lookup_host`(SSRF ガードの DNS 検証)は **tokio のリアクタ** を前提とするため、gpui の executor 上で直接 await すると `there is no reactor running` 系の panic になる。
- **解決パターン**: 調査実行は専用 `std::thread` を立てて `tokio::runtime::Runtime::new()?.block_on(...)` で回す(`runner.rs`)。GUI ↔ ワーカーの橋は `tokio::sync::mpsc::unbounded_channel`。**unbounded チャネルの `recv().await` はリアクタ不要(executor 非依存)** なので、gpui 側の `cx.spawn` 内で安全に await できる。bounded チャネルや `tokio::time` 系はこの性質を持たないので橋には使えない。
- GUI 側の受信ループは弱参照パターン:

```rust
cx.spawn(async move |this, cx| {            // this: WeakEntity<Self>
    while let Some(update) = rx.recv().await {
        if this.update(cx, |app, cx| app.apply_update(update, cx)).is_err() {
            break;                          // Err = ビューが破棄済み → ループ脱出
        }
    }
}).detach();
```

`this.update()` が `Result` を返すのは「Entity がもう存在しない」ケースの表現で、**エラー処理ではなくライフサイクル管理**。`.detach()` を忘れると Task が drop されて即座に止まる(これも無言)。

- core 側のイベント送出を `Box<dyn Fn(AgentEvent) + Send + Sync>` のコールバックにしておいたため、core は tokio にも gpui にも依存せず、GUI 側で「コールバック→チャネル送信」に変換するだけで済んだ。

## 5. macOS デスクトップアプリとしての挙動

- **ウィンドウを閉じてもプロセスは終わらない**(macOS の流儀がデフォルト)。明示的に `cx.on_window_closed(|cx| if cx.windows().is_empty() { cx.quit(); })` を仕込んだ。CLI 感覚で起動するツールではゾンビプロセス化するので必須。
- `cx.activate(true)` を呼ばないと起動してもウィンドウが前面に来ないことがある。
- `cargo run` で起動するのは **素のバイナリであって .app バンドルではない**。Dock 表示・アイコン・Info.plist・コード署名は別途バンドル化(cargo-bundle 等)が必要。配布しないローカルツールならバイナリ直起動で十分。
- **環境変数は起動元シェルから継承**される。API キー(`ANTHROPIC_API_KEY` 等)を使うなら「キーを export したターミナルから起動」が必要で、Finder/Dock から起動した場合は見えない。GUI アプリで秘密情報を扱う際の典型的な落とし穴。
- ウィンドウ生成は `WindowOptions { window_bounds: Some(WindowBounds::centered(size(px(1100.), px(760.)), cx)), titlebar: Some(TitlebarOptions { title: ... }), .. }`。公式例にならい `cx.spawn` 内で `cx.open_window` する。

## 6. GUI の検証・テストの現実

- **描画コードは実質ユニットテスト不能** と割り切り、テスト可能なロジック(履歴ストアの保存・一覧・削除・パス検証)を `history.rs` に純粋に切り出してそちらをテストした。GUI クレートでも「UI とロジックの分離」がテスト戦略のすべて。
- 自動での見た目検証も難しい: `screencapture` は画面収録権限、`osascript`(System Events)はアクセシビリティ権限が必要で、CI やエージェント環境では大抵ブロックされる。実用的な smoke test は「起動して数秒間プロセスが生存し、stderr に panic が出ない」こと。gpui はシェーダ・ウィンドウ生成に失敗すると即 abort するので、これだけでも壊滅的な退行は検出できる。
- デバッグビルドの gpui は依存クレート約400個・初回ビルド数分。**ワークスペースを分けて core/cli の開発時に GUI をビルド対象から外せるようにした**ことが開発速度に直結した(`cargo build -p agentic-search-cli` なら gpui に一切触れない)。
