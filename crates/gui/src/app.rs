//! Main window view: question input, run controls, live progress, report
//! viewer, and saved-report history.

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariants as _},
    input::{Input, InputState},
    select::{Select, SelectState},
    text::TextView,
    v_flex, ActiveTheme as _, Disableable as _, IndexPath, Sizable as _,
};

use agentic_search_core::config::LlmProviderKind;
use agentic_search_core::events::{self, AgentEvent, TraceRecord};

use crate::history::{HistoryEntry, HistoryStore, ReportMeta};
use crate::runner::{self, RunParams, RunUpdate};

const MAX_STATUS_LINES: usize = 200;
const PROVIDERS: [LlmProviderKind; 3] = [
    LlmProviderKind::Ollama,
    LlmProviderKind::Claude,
    LlmProviderKind::OpenAi,
];

/// Fallback list shown until the installed models are fetched from the
/// local Ollama server (or when it is unreachable).
const OLLAMA_FALLBACK_MODELS: &[&str] = &["llama3.2:3b", "gemma3:12b", "qwen3:8b", "qwen3:14b"];
const CLAUDE_MODELS: &[&str] = &["claude-sonnet-4-6", "claude-haiku-4-5", "claude-opus-4-8"];
const OPENAI_MODELS: &[&str] = &["gpt-4o-mini", "gpt-5-mini", "gpt-5"];

pub struct ResearchApp {
    question_input: Entity<InputState>,
    provider_index: usize,
    /// Model choices for the active provider; items follow provider switches.
    model_select: Entity<SelectState<Vec<SharedString>>>,
    /// Installed models reported by the local Ollama server (fallback list
    /// until fetched).
    ollama_models: Vec<SharedString>,
    max_iterations: u32,
    running: bool,
    status_lines: Vec<SharedString>,
    report: Option<SharedString>,
    /// Events of the current (or just finished) run, kept for the audit trace.
    trace: Vec<TraceRecord>,
    /// When true the main pane shows the run trace instead of the report.
    show_trace: bool,
    trace_markdown: Option<SharedString>,
    store: HistoryStore,
    history: Vec<HistoryEntry>,
    selected_stem: Option<String>,
}

impl ResearchApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let question_input = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .placeholder("調査したい質問を入力(例: Rust の async ランタイムの最新動向は?)")
        });
        let ollama_models: Vec<SharedString> = OLLAMA_FALLBACK_MODELS
            .iter()
            .map(|name| SharedString::from(*name))
            .collect();
        let model_select = cx
            .new(|cx| SelectState::new(ollama_models.clone(), Some(IndexPath::new(0)), window, cx));
        Self::spawn_ollama_model_fetch(window, cx);
        let store = HistoryStore::open_default().expect("failed to open report store");
        let history = store.list();
        Self {
            question_input,
            provider_index: 0,
            model_select,
            ollama_models,
            max_iterations: 2,
            running: false,
            status_lines: Vec::new(),
            report: None,
            trace: Vec::new(),
            show_trace: false,
            trace_markdown: None,
            store,
            history,
            selected_stem: None,
        }
    }

    fn provider(&self) -> LlmProviderKind {
        PROVIDERS[self.provider_index]
    }

    fn provider_label(&self) -> &'static str {
        match self.provider() {
            LlmProviderKind::Ollama => "LLM: Ollama (ローカル)",
            LlmProviderKind::Claude => "LLM: Claude",
            LlmProviderKind::OpenAi => "LLM: OpenAI",
        }
    }

    /// Model choices offered for a provider. Ollama uses the list fetched
    /// from the local server; cloud providers use curated current models.
    fn model_options(&self, provider: LlmProviderKind) -> Vec<SharedString> {
        match provider {
            LlmProviderKind::Ollama => self.ollama_models.clone(),
            LlmProviderKind::Claude => CLAUDE_MODELS.iter().map(|m| (*m).into()).collect(),
            LlmProviderKind::OpenAi => OPENAI_MODELS.iter().map(|m| (*m).into()).collect(),
        }
    }

    /// The model currently picked in the dropdown.
    fn selected_model(&self, cx: &Context<Self>) -> Option<String> {
        self.model_select
            .read(cx)
            .selected_value()
            .map(|model| model.to_string())
    }

    /// Replace the dropdown items after a provider switch (or after the
    /// Ollama list arrives), keeping the current choice when still present.
    fn refresh_model_items(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let items = self.model_options(self.provider());
        let previous = self.selected_model(cx);
        self.model_select.update(cx, |select, cx| {
            select.set_items(items.clone(), window, cx);
            let index = previous
                .and_then(|value| items.iter().position(|item| item.as_ref() == value))
                .unwrap_or(0);
            select.set_selected_index(Some(IndexPath::new(index)), window, cx);
        });
    }

    /// Ask the local Ollama server for its installed models in the
    /// background; updates the dropdown when the answer arrives.
    fn spawn_ollama_model_fetch(window: &mut Window, cx: &mut Context<Self>) {
        let mut rx = runner::fetch_ollama_models();
        cx.spawn_in(window, async move |this, cx| {
            if let Some(models) = rx.recv().await {
                if models.is_empty() {
                    return;
                }
                let _ = this.update_in(cx, |app, window, cx| {
                    app.ollama_models = models.into_iter().map(SharedString::from).collect();
                    if app.provider() == LlmProviderKind::Ollama {
                        app.refresh_model_items(window, cx);
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn push_status(&mut self, line: String) {
        self.status_lines.push(line.into());
        if self.status_lines.len() > MAX_STATUS_LINES {
            self.status_lines.remove(0);
        }
    }

    fn start_run(&mut self, cx: &mut Context<Self>) {
        if self.running {
            return;
        }
        let question = self.question_input.read(cx).value().trim().to_string();
        if question.is_empty() {
            self.push_status("質問を入力してください。".into());
            cx.notify();
            return;
        }
        self.running = true;
        self.status_lines.clear();
        self.report = None;
        self.trace.clear();
        self.show_trace = false;
        self.trace_markdown = None;
        self.selected_stem = None;
        self.push_status(format!("調査開始: {question}"));

        let mut rx = runner::start(RunParams {
            question,
            provider: self.provider(),
            model: self.selected_model(cx),
            max_iterations: self.max_iterations,
        });
        cx.spawn(async move |this, cx| {
            while let Some(update) = rx.recv().await {
                let finished = matches!(update, RunUpdate::Finished(_) | RunUpdate::Failed(_));
                if this
                    .update(cx, |app, cx| app.apply_update(update, cx))
                    .is_err()
                    || finished
                {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    fn apply_update(&mut self, update: RunUpdate, cx: &mut Context<Self>) {
        match update {
            RunUpdate::Event(event) => {
                self.push_status(describe_event(&event));
                self.trace.push(TraceRecord::now(event));
            }
            RunUpdate::Failed(message) => {
                self.running = false;
                self.push_status(format!("失敗: {message}"));
            }
            RunUpdate::Finished(outcome) => {
                self.running = false;
                self.push_status(format!(
                    "完了: {} findings / {} sources / {} 反復",
                    outcome.finding_count, outcome.source_count, outcome.iterations
                ));
                self.report = Some(outcome.markdown.clone().into());
                let meta = ReportMeta {
                    question: outcome.question.clone(),
                    saved_at: chrono::Local::now().to_rfc3339(),
                    freshness: outcome.freshness,
                    correctness: outcome.correctness,
                    coverage: outcome.coverage,
                    finding_count: outcome.finding_count,
                    source_count: outcome.source_count,
                    iterations: outcome.iterations,
                };
                match self
                    .store
                    .save(meta, &outcome.markdown, &events::to_jsonl(&self.trace))
                {
                    Ok(entry) => {
                        self.selected_stem = Some(entry.stem.clone());
                        self.history.insert(0, entry);
                    }
                    Err(err) => self.push_status(format!("保存に失敗: {err:#}")),
                }
            }
        }
        cx.notify();
    }

    fn open_entry(&mut self, stem: String, cx: &mut Context<Self>) {
        match self.store.load_markdown(&stem) {
            Ok(markdown) => {
                self.report = Some(markdown.into());
                self.selected_stem = Some(stem);
                self.show_trace = false;
                self.trace_markdown = None;
            }
            Err(err) => self.push_status(format!("読み込みに失敗: {err:#}")),
        }
        cx.notify();
    }

    /// Switch the main pane between the report and the run's audit trace.
    fn toggle_trace(&mut self, cx: &mut Context<Self>) {
        if self.show_trace {
            self.show_trace = false;
            cx.notify();
            return;
        }
        let records = match &self.selected_stem {
            Some(stem) => match self.store.load_trace(stem) {
                Ok(jsonl) => events::from_jsonl(&jsonl),
                Err(err) => {
                    self.push_status(format!("トレースの読み込みに失敗: {err:#}"));
                    cx.notify();
                    return;
                }
            },
            None => self.trace.clone(),
        };
        self.trace_markdown = Some(format_trace(&records).into());
        self.show_trace = true;
        cx.notify();
    }

    fn delete_entry(&mut self, stem: String, cx: &mut Context<Self>) {
        if let Err(err) = self.store.delete(&stem) {
            self.push_status(format!("削除に失敗: {err:#}"));
        } else {
            self.history.retain(|entry| entry.stem != stem);
            if self.selected_stem.as_deref() == Some(stem.as_str()) {
                self.selected_stem = None;
                self.report = None;
                self.show_trace = false;
                self.trace_markdown = None;
            }
        }
        cx.notify();
    }

    fn cycle_provider(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.provider_index = (self.provider_index + 1) % PROVIDERS.len();
        self.refresh_model_items(window, cx);
        cx.notify();
    }

    fn adjust_iterations(&mut self, delta: i64, cx: &mut Context<Self>) {
        self.max_iterations = (self.max_iterations as i64 + delta).clamp(1, 8) as u32;
        cx.notify();
    }

    fn render_sidebar(&self, cx: &Context<Self>) -> impl IntoElement {
        let items = self.history.iter().enumerate().map(|(index, entry)| {
            let stem = entry.stem.clone();
            let stem_for_delete = stem.clone();
            let selected = self.selected_stem.as_deref() == Some(entry.stem.as_str());
            div()
                .id(("history", index))
                .p_2()
                .rounded_md()
                .cursor_pointer()
                .when(selected, |this| this.bg(cx.theme().accent))
                .hover(|this| this.bg(cx.theme().accent))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.open_entry(stem.clone(), cx);
                }))
                .child(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(truncate_chars(&entry.meta.question, 36)),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!(
                                    "{} | 鮮{} 正{} 網{}",
                                    truncate_chars(&entry.meta.saved_at, 10),
                                    entry.meta.freshness,
                                    entry.meta.correctness,
                                    entry.meta.coverage
                                ))
                                .child(
                                    Button::new(("delete", index))
                                        .ghost()
                                        .xsmall()
                                        .label("削除")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.delete_entry(stem_for_delete.clone(), cx);
                                        })),
                                ),
                        ),
                )
        });

        v_flex()
            .w(px(280.))
            .h_full()
            .flex_shrink_0()
            .border_r_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().sidebar)
            .child(
                div()
                    .p_3()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(format!("履歴 ({})", self.history.len())),
                    )
                    .child(
                        Button::new("open-folder")
                            .ghost()
                            .xsmall()
                            .label("Finderで開く")
                            .on_click(cx.listener(|this, _, _, _| {
                                let _ = std::process::Command::new("open")
                                    .arg(this.store.dir())
                                    .spawn();
                            })),
                    ),
            )
            .child(
                div()
                    .id("history-list")
                    .flex_1()
                    .overflow_y_scroll()
                    .p_2()
                    .child(v_flex().gap_1().children(items)),
            )
    }

    fn render_controls(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap_2()
            .child(
                Button::new("provider")
                    .outline()
                    .label(self.provider_label())
                    .on_click(cx.listener(|this, _, window, cx| this.cycle_provider(window, cx))),
            )
            .child(
                Select::new(&self.model_select)
                    .small()
                    .w(px(220.))
                    .menu_width(px(280.))
                    .placeholder("モデルを選択"),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        Button::new("iter-minus")
                            .outline()
                            .label("-")
                            .on_click(cx.listener(|this, _, _, cx| this.adjust_iterations(-1, cx))),
                    )
                    .child(
                        div()
                            .text_sm()
                            .child(format!("反復 {}", self.max_iterations)),
                    )
                    .child(
                        Button::new("iter-plus")
                            .outline()
                            .label("+")
                            .on_click(cx.listener(|this, _, _, cx| this.adjust_iterations(1, cx))),
                    ),
            )
            .child(div().flex_1())
            .child(
                Button::new("run")
                    .primary()
                    .label(if self.running {
                        "調査中…"
                    } else {
                        "調査開始"
                    })
                    .loading(self.running)
                    .disabled(self.running)
                    .on_click(cx.listener(|this, _, _, cx| this.start_run(cx))),
            )
    }

    fn render_status(&self, cx: &Context<Self>) -> impl IntoElement {
        let recent = self
            .status_lines
            .iter()
            .rev()
            .take(6)
            .rev()
            .cloned()
            .collect::<Vec<_>>();
        div()
            .id("status")
            .max_h(px(120.))
            .overflow_y_scroll()
            .p_2()
            .rounded_md()
            .bg(cx.theme().muted)
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(
                v_flex()
                    .gap_0p5()
                    .children(recent.into_iter().map(|line| div().child(line))),
            )
    }

    /// Header row above the main pane: current view label + trace toggle.
    fn render_view_toggle(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(if self.show_trace {
                        "実行トレース(監査ログ)"
                    } else {
                        "レポート"
                    }),
            )
            .child(
                Button::new("toggle-trace")
                    .outline()
                    .xsmall()
                    .label(if self.show_trace {
                        "レポートを表示"
                    } else {
                        "トレースを表示"
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_trace(cx))),
            )
    }

    /// Main pane. The container gives the `TextView` a definite height
    /// (flex_1 + min_h(0)), which its scrollable mode requires; the view then
    /// virtualizes content and shows its own scrollbar. Markdown links open
    /// in the default browser (built into gpui-component via `cx.open_url`).
    fn render_report(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = if self.show_trace {
            self.trace_markdown.clone().map(|md| ("trace-view", md))
        } else {
            self.report.clone().map(|md| ("report-view", md))
        };
        let body: AnyElement = match content {
            Some((id, markdown)) => TextView::markdown(id, markdown, window, cx)
                .scrollable(true)
                .selectable(true)
                .p_4()
                .into_any_element(),
            None => div()
                .p_4()
                .text_color(cx.theme().muted_foreground)
                .child("レポートはまだありません。質問を入力して「調査開始」を押してください。")
                .into_any_element(),
        };
        div()
            .flex_1()
            .min_h(px(0.))
            .overflow_hidden()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .child(body)
    }
}

impl Render for ResearchApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.render_sidebar(cx))
            .child(
                v_flex()
                    .flex_1()
                    .h_full()
                    .p_4()
                    .gap_3()
                    .child(Input::new(&self.question_input).h(px(64.)))
                    .child(self.render_controls(cx))
                    .when(!self.status_lines.is_empty(), |this| {
                        this.child(self.render_status(cx))
                    })
                    .when(self.report.is_some() || !self.trace.is_empty(), |this| {
                        this.child(self.render_view_toggle(cx))
                    })
                    .child(self.render_report(window, cx)),
            )
    }
}

fn describe_event(event: &AgentEvent) -> String {
    match event {
        AgentEvent::PlanReady { queries } => {
            format!(
                "計画完了: {} クエリ — {}",
                queries.len(),
                queries.join(" / ")
            )
        }
        AgentEvent::QueryStarted { query } => format!("検索中: {query}"),
        AgentEvent::PageProcessed { url, new_findings } => {
            format!("取得: {url}(新規 {new_findings} 件)")
        }
        AgentEvent::IterationDone {
            iteration,
            new_findings,
            total_findings,
        } => format!("反復 {iteration} 完了: 新規 {new_findings} 件 / 計 {total_findings} 件"),
        AgentEvent::EvaluationDone {
            iteration,
            evaluation,
        } => format!(
            "自己評価(反復 {iteration}): 鮮度 {} / 正確性 {} / 網羅性 {}{}",
            evaluation.freshness.score,
            evaluation.correctness.score,
            evaluation.coverage.score,
            if evaluation.sufficient() {
                " — 十分と判定"
            } else {
                " — 追加調査へ"
            }
        ),
    }
}

/// Render trace records as a readable Markdown audit log. Evaluation events
/// expand into per-axis issues and proposed follow-up queries so a reviewer
/// can see why the agent kept (or stopped) searching.
fn format_trace(records: &[TraceRecord]) -> String {
    if records.is_empty() {
        return "実行トレースはありません(トレース機能の追加前に作成されたレポートです)。"
            .to_string();
    }
    let mut out = String::from("## 実行トレース\n\n");
    for record in records {
        out.push_str(&format!(
            "- `{}` {}\n",
            record.timestamp.format("%H:%M:%S"),
            describe_event(&record.event)
        ));
        if let AgentEvent::EvaluationDone { evaluation, .. } = &record.event {
            let axes = [
                ("鮮度", &evaluation.freshness),
                ("正確性", &evaluation.correctness),
                ("網羅性", &evaluation.coverage),
            ];
            for (axis, review) in axes {
                for issue in &review.issues {
                    out.push_str(&format!("  - {axis}の指摘: {issue}\n"));
                }
            }
            for query in &evaluation.followup_queries {
                out.push_str(&format!("  - 追加クエリ案: {query}\n"));
            }
        }
    }
    out
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    match text.char_indices().nth(max_chars) {
        Some((index, _)) => format!("{}…", &text[..index]),
        None => text.to_string(),
    }
}
