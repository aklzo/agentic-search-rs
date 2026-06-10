# AI エージェントアーキテクチャ調査(2025年後半〜2026年)

最終更新: 2026-06-10

## 0. 前提:2025〜2026年の全体像

この期間のエージェント設計を理解するうえで重要な前提が3つある。

1. **「ワークフロー」と「エージェント」の区別**
   Anthropic の整理([Building Effective Agents](https://www.anthropic.com/engineering/building-effective-agents))以降、コードで制御フローを固定する「ワークフロー」(プロンプトチェーン、ルーティング、並列化など)と、LLM 自身が動的にツール使用と手順を決める「エージェント」を区別するのが業界標準になった。本ノートでは後者を中心に扱うが、実運用ではワークフローとエージェントのハイブリッドが大半である。

2. **Agent Harness(エージェントハーネス)という概念の定着**
   モデル単体ではなく「モデル+ツール実行+コンテキスト管理+エラー処理+権限制御」をまとめたランタイムを Agent Harness と呼ぶ。Claude Code / Codex CLI / Cursor などの性能差は、モデル差と同じくらいハーネス設計の差で説明されるようになった([解説](https://www.mindstudio.ai/blog/what-is-agent-harness-architecture-explained))。

3. **プロンプトエンジニアリングからコンテキストエンジニアリングへ**
   長時間タスクでは「コンテキストウィンドウに何を入れ、何を捨て、いつ圧縮するか」が性能を支配する。Anthropic の [Effective Context Engineering for AI Agents](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents) がこの転換を象徴する文書。コンテキストが長くなるほど recall が劣化する「context rot」が共通課題として認識されている。

---

## 1. シングルエージェント・アーキテクチャ

### 1.1 ReAct 型エージェンティックループ(Tool-Use Loop)

**構造**: 「推論 → ツール呼び出し → 結果観測 → 推論…」を目標達成まで繰り返す単一ループ。原型は ReAct 論文(Yao et al., 2022, [arXiv:2210.03629](https://arxiv.org/abs/2210.03629))だが、2025〜26年は推論特化モデル(extended thinking)+大量のツールという形で進化し、**現在の本番エージェントの最頻出パターン**。

- **ユースケース**: コーディング、調査、運用作業の自動化など、手順を事前に固定できないタスク全般。
- **応用例(プロダクト)**:
  - **Claude Code**(Anthropic): ターミナル常駐型。リポジトリ読解 → 計画 → シェル実行 → テスト → 修正のループ。`CLAUDE.md` による永続指示、MCP によるツール拡張。
  - **OpenAI Codex / Codex CLI**、**Google Jules / Gemini CLI**、**Cursor**、**Cognition Devin**。
  - **Manus**: 汎用タスクエージェント。仮想マシン内でのループ実行+ファイルシステムを外部メモリとして使う設計が特徴。

### 1.2 Plan-and-Execute(計画・実行分離)

**構造**: プランナーがタスク分解した計画(順序付きステップ列や DAG)を出力し、エグゼキューターが1ステップずつ実行する2フェーズ構成。再計画(re-planning)ステップを挟むことが多い。実行側に小型・安価なモデルを使えるためコスト効率がよい。

- **ユースケース**: ステップ数が多く全体方針を先に固めたい長期タスク。コスト最適化(計画=高性能モデル、実行=軽量モデル)。
- **応用例**: Devin の計画モード、LangChain/LangGraph の plan-and-execute テンプレート、Claude Code の Plan Mode・タスクリスト機能。POLARIS のように計画を「型付きの DAG 合成+バリデータでゲートされた実行」として統制する研究も出ている([2026年タクソノミー](https://www.digitalapplied.com/blog/agent-architecture-patterns-taxonomy-2026))。

### 1.3 Reflection / Evaluator-Optimizer(自己評価ループ)

**構造**: 生成役と評価役(同一モデルの別ロールでも別モデルでもよい)を分け、「生成 → 批評 → 修正」を収束まで回す。原型は Reflexion(Shinn et al., 2023, [arXiv:2303.11366](https://arxiv.org/abs/2303.11366))。

- **ユースケース**: 明確な評価基準があるタスク。コードレビュー付き生成、文章推敲、翻訳品質改善、テスト駆動の修正ループ。
- **応用例**: コーディングエージェントの「テスト失敗 → 自己修正」ループ(Claude Code、OpenHands)、LLM-as-a-Judge を組み込んだ生成パイプライン。

### 1.4 CodeAct(コードをアクション空間にする)

**構造**: ツールを個別 API として呼ぶ代わりに、エージェントが Python などの実行可能コードを書き、サンドボックスで実行した結果を観測する。1回のアクションで複合操作(ループ・条件分岐・データ加工)ができ、JSON ツール呼び出しよりステップ数が大幅に減る(CodeAct, [arXiv:2402.01030](https://arxiv.org/abs/2402.01030))。

- **ユースケース**: データ分析、ファイル加工、複数 API の組み合わせ処理。
- **応用例**: **OpenHands**(旧 OpenDevin)、**Hugging Face smolagents**、**Manus**(CodeAct 系統の実行モデル)。MCP ツールをコード経由で呼ぶ「code execution with MCP」パターンとして Claude Agent SDK にも波及。

### 1.5 Computer Use / GUI エージェント

**構造**: スクリーンショット(または DOM/アクセシビリティツリー)を観測し、クリック・キー入力を行動として出力する知覚-行動ループ。API が存在しない既存 UI をそのまま操作できる。

- **ユースケース**: API 化されていない業務システムの操作、ブラウザ上の予約・購買・フォーム入力、RPA の置き換え、E2E テスト自動化。
- **応用例**: **OpenAI Operator / ChatGPT Agent**、**Anthropic Computer Use / Claude for Chrome**、**Google Project Mariner**。2026年時点では「スクリプト化しにくいワークフロー」の自動化手段として実用域に入った([Webfuse, Agentic Coding in 2026](https://www.webfuse.com/blog/agentic-coding-in-2026))。ライブの本番サイト283タスクで評価する ClawBench など専用ベンチマークも登場([VoltAgent 論文コレクション](https://github.com/VoltAgent/awesome-ai-agent-papers))。

### 1.6 メモリ拡張型(OS スタイルの階層メモリ)

**構造**: コンテキストウィンドウを「RAM」、外部ストレージを「ディスク」とみなし、エージェント自身がツールで自分のメモリをページイン/アウトする。原型は MemGPT([arXiv:2310.08560](https://arxiv.org/abs/2310.08560))。

- **ユースケース**: 長期間継続するパーソナルアシスタント、顧客ごとの履歴を保持するサポートエージェント、セッションをまたぐ作業エージェント。
- **応用例**: **Letta**(MemGPT の製品化)、**Mem0**([arXiv:2504.19413](https://arxiv.org/abs/2504.19413)、本番向けスケーラブル長期メモリ)、ChatGPT / Claude のメモリ機能、Claude Code のファイルベース永続メモリ。

---

## 2. マルチエージェント・アーキテクチャ

> 2026年時点の実態として、本番システムの主流は「オーケストレーター・ワーカー(hub-and-spoke)」であり、その他のパターンは要件に応じて組み合わせる「コンポーザブルな部品」と捉えられている([Agents Index](https://agentsindex.ai/blog/multi-agent-systems), [Gurusup](https://gurusup.com/blog/agent-orchestration-patterns))。

### 2.1 オーケストレーター・ワーカー(Supervisor-Worker / Hub-and-Spoke)

**構造**: 単一のオーケストレーターがタスクを分解し、専門化されたワーカーエージェントに委譲して結果を集約する。ワーカー同士は直接通信せず、調整はすべてオーケストレーター経由。**各ワーカーが独立したコンテキストウィンドウを持つ**ことが本質で、コンテキスト分離(context isolation)の手段としても使われる。

- **ユースケース**: 並列化できる調査・探索タスク、トークン量が単一コンテキストに収まらないタスク、専門性の異なるサブタスクの統合。
- **応用例**:
  - **Anthropic Research(Deep Research)**: リードエージェントが計画を立て、3〜5個のサブエージェントを並列に生成して検索させ、結果を統合+引用チェック。Opus 4 オーケストレーター+Sonnet 4 サブエージェント構成で、単体 Opus 4 比 **90.2% の性能向上**を報告([Anthropic engineering blog](https://www.anthropic.com/engineering/multi-agent-research-system))。
  - OpenAI / Google の Deep Research 系プロダクト(体系的サーベイ: [Deep Research: A Systematic Survey](https://arxiv.org/pdf/2512.02038))。
  - Claude Code のサブエージェント機構(Explore / Plan / general-purpose などをタスク単位で派遣)。

### 2.2 階層型(Hierarchical)

**構造**: オーケストレーター・ワーカーを多段に重ねる。ドメインごとのエージェント群(顧客サポート、営業、IT 運用など)をそれぞれスーパーバイザーが管理し、スーパーバイザーが上位コーディネーターに報告する。

- **ユースケース**: 50体以上のエージェントを複数業務ドメインにまたがって運用するエンタープライズ規模では事実上唯一の選択肢とされる([Kore.ai](https://www.kore.ai/blog/choosing-the-right-orchestration-pattern-for-multi-agent-systems), [Augment Code](https://www.augmentcode.com/guides/swarm-vs-supervisor))。
- **応用例**: Salesforce **Agentforce**、IBM watsonx Orchestrate などのエンタープライズエージェント基盤。研究例として地球科学データアーカイブの自律探索([arXiv:2602.21351](https://arxiv.org/pdf/2602.21351))。

### 2.3 グラフオーケストレーション(状態グラフ / ステートマシン)

**構造**: エージェントを有向グラフのノードとして配置し、制御フロー(エッジ)・状態遷移・チェックポイントをコードで明示的に定義する。LLM の自律性をノード内に閉じ込め、ノード間の遷移は決定的に制御するため、再現性・監査性・human-in-the-loop の挿入が容易。

- **ユースケース**: 規制業種や金融など決定性・監査性が要る業務フロー、失敗時に特定ノードから再開したい長時間処理、承認ステップを挟むワークフロー。
- **応用例**: **LangGraph**(代表的実装。checkpoint による永続化・再開)、**Microsoft Agent Framework**(AutoGen と Semantic Kernel を統合した後継、2025年後半)、**Google ADK** のワークフローエージェント(Sequential/Parallel/Loop)。

### 2.4 スウォーム / ハンドオフ / メッシュ(分散型)

**構造**: 中央監督なしの対等なエージェント群。あるエージェントが判断して別のエージェントに会話ごと制御を引き渡す(handoff)、または共有黒板(blackboard)・メッセージバスに読み書きして緩く協調する。

- **ユースケース**: 問い合わせのトリアージ→専門エージェントへの引き継ぎ(カスタマーサポート)、探索的な並列データ収集、所有者の異なるエージェント同士の疎結合な連携。
- **応用例**: **OpenAI Agents SDK** の handoff プリミティブ(2025年3月、Swarm の後継)、LangGraph Swarm。中央ボトルネックがない代わりにデバッグ・収束保証が難しく、単体で本番採用される例は少ない([DEV Community の比較](https://dev.to/jose_gurusup_dev/agent-orchestration-patterns-swarm-vs-mesh-vs-hierarchical-vs-pipeline-b40))。

### 2.5 パイプライン(直列ステージ)

**構造**: 各エージェントが前段の出力を入力として順に処理する。ワークフロー寄りだが、各ステージ内部はエージェンティックでもよい。

- **ユースケース**: 文書処理(抽出→正規化→検証→登録)、コンテンツ制作(リサーチ→執筆→校閲)、ETL 的なデータ処理。
- **応用例**: CrewAI の sequential process、各種ドキュメント処理 SaaS。ステージ単位の品質ゲートを置きやすいのが利点。

### 2.6 ディベート / アンサンブル(合議・検証型)

**構造**: 複数エージェントが同じ問題に独立に取り組み、相互批評・投票・コンセンサスで最終出力を決める。精度・信頼性をコストで買うパターン。

- **ユースケース**: 高リスク判断の検証、LLM-as-a-Judge の頑健化、幻覚の低減。
- **応用例**: コンセンサス駆動の分解実行でエンタープライズ級の信頼性を狙う [The Six Sigma Agent](https://arxiv.org/pdf/2601.22290)(2026)など。

### 2.7 ハイブリッド(実運用の現実解)

実際の本番システムは単一パターンではなく、「階層型の末端チームが内部ではメッシュ」「パイプラインの1ステージが並列スウォームを起動」のような**組み合わせ**になる。パターンはコンポーザブルであり、サブシステムごとの要件で選ぶのが2026年のコンセンサス([decodethefuture](https://decodethefuture.org/en/multi-agent-systems-explained/))。

### マルチエージェント化の判断基準(重要な留保)

UC Berkeley の MAST 論文(後述)が示す通り、**マルチエージェント化は単一エージェントに対して必ずしも性能向上をもたらさない**。Anthropic も「並列化可能で読み込みトークンが大きいタスク(調査系)には有効だが、エージェント間で密に文脈共有が必要なタスク(コーディングの大半)には不向き」と整理している。「ステップ2がステップ1の全詳細を必要とするならサブエージェントは分離の利益なくオーバーヘッドだけ足す」([philschmid, Context Engineering Part 2](https://www.philschmid.de/context-engineering-part-2))。

---

## 3. アーキテクチャを支える基盤レイヤ(2025後半〜2026の標準化)

| レイヤ | 標準/技術 | 概要 |
|---|---|---|
| ツール接続 | **MCP**(Model Context Protocol) | エージェント⇔ツール/データ接続の事実上の標準。 |
| エージェント間通信 | **A2A**(Agent2Agent) | 組織・ベンダーをまたぐエージェント間の発見・タスク委譲プロトコル。 |
| ガバナンス | **AAIF**(Agentic AI Foundation) | 2025年12月に Linux Foundation 傘下で発足。OpenAI・Anthropic・Google・Microsoft・AWS・Block が共同創設し、MCP と A2A の両方が同財団のプロジェクトに。「MCP=ツール統合、A2A=エージェント調整」の2層モデルが参照アーキテクチャとして固まりつつある([Zylos Research](https://zylos.ai/research/2026-03-26-agent-interoperability-protocols-mcp-a2a-acp-convergence/))。 |
| 実行分離 | サンドボックス | コード実行・ブラウザ操作を VM/コンテナに隔離するのが本番の前提。ツール出力のサンドボックス整形はコンテキスト削減策としても最重要級([Fundesk](https://www.fundesk.io/context-engineering-techniques-ai-coding-agents-2026))。 |
| SDK / フレームワーク | Claude Agent SDK、OpenAI Agents SDK、Google ADK、Microsoft Agent Framework、LangGraph、CrewAI | 2025年に各社の公式 SDK が出揃い、ハーネス自作からの移行が進んだ。 |
| 運用(AgentOps) | トレーシング・評価・コスト管理 | LangSmith、Langfuse、Maxim 等。長期実行エージェントの可観測性が独立した分野に。 |

---

## 4. 補足:エージェントの課題と、それに対応する最新論文

ACE のように「エージェントの構造的な課題に対する仕組み上の工夫」を提案する論文を、課題別に整理する。

### 4.1 課題:コンテキストの劣化(context rot / context collapse)と長期タスク

長時間ループではコンテキストが肥大化して recall が劣化し、逆に素朴な要約圧縮は重要な詳細を不可逆に失う。このジレンマへの提案群:

- **ACE: Agentic Context Engineering**(Stanford ほか, 2025年10月, [arXiv:2510.04618](https://arxiv.org/abs/2510.04618))
  コンテキストを「進化するプレイブック」として扱い、**Generator(実行)/ Reflector(振り返り)/ Curator(編集)** の3役で戦略を蓄積・精錬する。全文を書き直す方式が引き起こす **context collapse**(反復書き換えによる詳細の侵食)と **brevity bias**(簡潔さ優先でドメイン知見が脱落)を、構造化された増分更新で回避する。エージェントタスクで +10.6%、金融タスクで +8.6%。AppWorld リーダーボードでは ReAct+ACE(オープンな DeepSeek-V3.1)が GPT-4.1 ベースの IBM CUGA に並んだ。重み更新なしの自己改善という点で本節の代表格。
- **AgentFold**(Alibaba Tongyi Lab, 2025年10月, [arXiv:2510.24699](https://arxiv.org/abs/2510.24699))
  コンテキストを「受動的なログ」ではなく「能動的に彫刻する認知ワークスペース」と捉え、各ステップで履歴を多スケールに**折りたたむ(folding)**操作を学習する。細部を保つ細粒度の凝縮と、サブタスク丸ごとの深い統合を使い分ける。30B-A3B モデルで BrowseComp 36.2% を達成し、DeepSeek-V3.1-671B や o4-mini を上回った。
- **Context-Folding / 分岐型コンテキスト管理**(2025): サブタスクを分岐(branch)して実行し、復帰時に折りたたむ枠組み。コンテキスト予算内に収める挙動を RL で学習する系列。
- **ACON**(2025): 長い対話履歴をどう要約すべきかの「圧縮ガイドライン」自体を失敗事例から自然言語で学習し、RL やファインチューニングなしでコンテキストを最大54%削減。
- **プロバイダ側の対応**: Anthropic のコンテキスト編集・**compaction API**(2026年1月)など、圧縮がプラットフォーム機能として製品化される段階に入った([Claude Cookbook](https://platform.claude.com/cookbook/tool-use-context-engineering-context-engineering-tools))。

### 4.2 課題:経験から学習しない(同じ失敗の繰り返し)

エージェントはタスクごとに使い捨てで、蓄積した相互作用履歴から学ばない。これに対するメモリ機構の提案群:

- **ReasoningBank + MaTTS**(Google Research, 2025年9月, [arXiv:2509.25140](https://arxiv.org/abs/2509.25140))
  生のトラジェクトリや成功ルーチンではなく、**成功と失敗の両方から「戦略レベル」の推論メモリ**を蒸留して蓄積。テスト時に関連メモリを取得して行動に反映し、新しい学びを書き戻す。さらに **Memory-aware Test-Time Scaling(MaTTS)** で1タスクに多様な試行を割り当て、対照的な経験からより質の高いメモリを合成する。「メモリ × テスト時スケーリング」という新しい軸を提示した。
- **Mem0**([arXiv:2504.19413](https://arxiv.org/abs/2504.19413)): 本番運用向けのスケーラブルな長期メモリ。抽出→統合(追加/更新/削除の判断)→検索のパイプラインとグラフ版を提案。
- **A-MEM**(2025): Zettelkasten に着想を得て、メモリ同士を動的にリンク・再組織化するエージェンティックメモリ。
- **Memento**(2025, [arXiv:2508.16153](https://arxiv.org/abs/2508.16153)): ケースベース推論で過去エピソードを記憶バンクに保持し、**LLM の重みを更新せずに**継続的に方策を改善する枠組み。
- **Dynamic Cheatsheet**(2025, [arXiv:2504.07952](https://arxiv.org/abs/2504.07952)): テスト時に再利用可能な知見・コード片を「カンニングペーパー」として蓄積する適応メモリ。ACE の先行研究の一つ。
- サーベイ: **Memory in the Age of AI Agents: A Survey**([論文リスト](https://github.com/Shichun-Liu/Agent-Memory-Paper-List))、メモリ評価の限界を実証分析した [Anatomy of Agentic Memory](https://arxiv.org/html/2602.19320v1)(2026)。

### 4.3 課題:重み更新なしの自己改善・スキル獲得

ファインチューニングはコスト・運用面で重く、プロプライエタリモデルには適用できない。「学習をパラメータ空間ではなくコンテキスト/スキル空間で行う」方向:

- **Training-Free GRPO**(Tencent, 2025年10月, [arXiv:2510.08191](https://arxiv.org/abs/2510.08191)): GRPO の「グループ内比較から学ぶ」発想を重み更新なしに転用し、経験知識をコンテキスト(トークン事前分布)として蓄積する。ACE と並ぶ「context as parameters」路線。
- **スキルライブラリ系**: Minecraft エージェント Voyager([arXiv:2305.16291](https://arxiv.org/abs/2305.16291))が原型(成功したコードを名前付き関数として保存・再利用)。2025〜26年は **SAGE**(Sequential Rollout でスキルを検証・蓄積しつつ GRPO で強化)、**AutoSkill**([arXiv:2603.01145](https://arxiv.org/html/2603.01145v2)、経験駆動の生涯学習)、**MUSE-Autoskill**([arXiv:2605.27366](https://arxiv.org/html/2605.27366v1))、能力ギャップ検出時にツールを自作する **Alita** / **Live-SWE-Agent** 系へ発展。プロダクト側では Claude の Skills(手順書+スクリプトの再利用可能パッケージ)が同じ思想の製品化。
- サーベイ: **A Survey of Self-Evolving Agents: What, When, How, and Where to Evolve**([arXiv:2507.21046](https://arxiv.org/abs/2507.21046))が「何を・いつ・どう進化させるか」の軸で2022〜2025年の系譜を整理。実環境でのスキル利用を測るベンチマーク([arXiv:2604.04323](https://arxiv.org/pdf/2604.04323))も整備されつつある。

### 4.4 課題:マルチエージェント協調の失敗

- **MAST: Why Do Multi-Agent LLM Systems Fail?**(UC Berkeley, 2025, [arXiv:2503.13657](https://arxiv.org/abs/2503.13657), NeurIPS 2025 Datasets & Benchmarks)
  7つの主要 MAS フレームワークの1600以上の注釈付きトレースから、**14の失敗モードを3カテゴリ**——(1) 仕様の問題(役割・タスク定義の曖昧さ)、(2) エージェント間の不整合(情報の握りつぶし、役割逸脱、ステップ反復)、(3) タスク検証の不備(検証の欠落・不完全な検証)——に体系化した初の実証的タクソノミー。多くの失敗は LLM の能力不足ではなく**システム設計(仕様と調整プロトコル)の問題**であると指摘し、マルチエージェント設計の事実上のチェックリストになっている。
- **協調構造の改善**: 固定トポロジーではなく推論ラウンドごとに意味的マッチングでエージェント間接続を再配線する **DyTopo**(2026)、ノイズの多いエージェント間メッセージを共形予測でフィルタする **CommCP**(2026)、並列エージェントチーム間で「何を共有すべきか」を選択的に学ぶ **Learning to Share**(2026)([VoltAgent の2026年論文コレクション](https://github.com/VoltAgent/awesome-ai-agent-papers)より)。
- **クレジット割り当て**: マルチエージェントのファインチューニングに行動単位のプロセス報酬を導入する **Scaling Multiagent Systems with Process Rewards**(2026)。

### 4.5 課題:検証・信頼性・評価

「もっともらしく間違える」エージェントを本番投入するための検証機構:

- **TrajAD**(2026): 実行トラジェクトリの誤りを実行時に検出する専用バリデータ。誤り箇所への精密なロールバック&リトライを可能にする。
- **CAR-bench**(2026): 曖昧さの下でのエージェントの一貫性・不確実性処理・**自己の能力限界の認識**を評価。
- **ROPE**(2026): MCTS ベースの報酬で欠陥のある推論ステップを特定・修正するプロセス監視 RL。
- ガードレール/ランタイム統制: エージェントの OS リソースを cgroup 的に制御する [AgentCgroup](https://arxiv.org/pdf/2602.09345)(2026)、配備済みエージェントの安全機能を体系記録する [The 2025 AI Agent Index](https://arxiv.org/pdf/2602.17753)。

### 4.6 課題マップ(まとめ)

| 課題 | 代表的な仕組み・論文 | アプローチの本質 |
|---|---|---|
| コンテキスト劣化・長期タスク | ACE、AgentFold、ACON、compaction API | コンテキストを「編集対象の構造物」として増分管理 |
| 経験からの学習 | ReasoningBank+MaTTS、Mem0、Memento | 戦略レベルの記憶を蒸留し、テスト時に読み書き |
| 重み更新なしの改善 | ACE、Training-Free GRPO、スキルライブラリ | 学習をパラメータ空間からコンテキスト/スキル空間へ移す |
| マルチエージェント協調の失敗 | MAST、DyTopo、CommCP | 失敗の体系化と、仕様・通信・トポロジーの設計改善 |
| 検証・信頼性 | TrajAD、ROPE、Six Sigma Agent | プロセスレベルの検証・ロールバック・合議 |

---

## 5. 全体の所感(2026年中盤時点)

- **アーキテクチャは収斂しつつある**: 「強い単一ループ(ReAct+ハーネス)」を基本に、コンテキスト分離が必要なときだけオーケストレーター・ワーカーを足し、決定性が必要な部分はグラフ/ワークフローで固める——という使い分けがベストプラクティスとして定着した。
- **競争軸はモデルからハーネスとコンテキスト管理へ**: ACE・AgentFold・ReasoningBank に共通するのは「モデルの重みではなく、モデルに見せる情報の構造を進化させる」という発想であり、2025年後半以降の最重要トレンド。
- **マルチエージェントは銀の弾丸ではない**: MAST が示す通り失敗の多くは設計起因。並列性とコンテキスト分離に利益があるタスクに限定して採用するのが現実解。
- **標準化の完了が前提を変えた**: MCP/A2A の Linux Foundation(AAIF)移管により、単一ベンダー内に閉じないエージェント間連携が2026年の設計前提になった。

## 主要ソース

- [Anthropic — How we built our multi-agent research system](https://www.anthropic.com/engineering/multi-agent-research-system)
- [Anthropic — Effective context engineering for AI agents](https://www.anthropic.com/engineering/effective-context-engineering-for-ai-agents)
- [Agentic AI: a comprehensive survey of architectures, applications, and future directions(Springer, 2025)](https://link.springer.com/article/10.1007/s10462-025-11422-4)
- [Agent Architecture Patterns: 2026 Taxonomy(Digital Applied)](https://www.digitalapplied.com/blog/agent-architecture-patterns-taxonomy-2026)
- [Multi-Agent Systems: How They Work, When to Use Them(Agents Index)](https://agentsindex.ai/blog/multi-agent-systems)
- [Agent Orchestration Patterns: Swarm vs Mesh vs Hierarchical vs Pipeline(DEV Community)](https://dev.to/jose_gurusup_dev/agent-orchestration-patterns-swarm-vs-mesh-vs-hierarchical-vs-pipeline-b40)
- [Agent Interoperability Protocols 2026: MCP, A2A, ACP(Zylos Research)](https://zylos.ai/research/2026-03-26-agent-interoperability-protocols-mcp-a2a-acp-convergence/)
- [What Is an Agent Harness?(MindStudio)](https://www.mindstudio.ai/blog/what-is-agent-harness-architecture-explained)
- [VoltAgent — awesome-ai-agent-papers(2026年論文コレクション)](https://github.com/VoltAgent/awesome-ai-agent-papers)
- 論文: [ACE (arXiv:2510.04618)](https://arxiv.org/abs/2510.04618) / [MAST (arXiv:2503.13657)](https://arxiv.org/abs/2503.13657) / [ReasoningBank (arXiv:2509.25140)](https://arxiv.org/abs/2509.25140) / [AgentFold (arXiv:2510.24699)](https://arxiv.org/abs/2510.24699) / [Mem0 (arXiv:2504.19413)](https://arxiv.org/abs/2504.19413) / [Self-Evolving Agents Survey (arXiv:2507.21046)](https://arxiv.org/abs/2507.21046) / [Deep Research: A Systematic Survey (arXiv:2512.02038)](https://arxiv.org/pdf/2512.02038)
