//! Main window view: question input, run controls, live progress, report
//! viewer, and saved-report history.

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariants as _},
    input::{Input, InputState},
    text::TextView,
    v_flex, ActiveTheme as _, Disableable as _, Sizable as _,
};

use agentic_search_core::config::LlmProviderKind;
use agentic_search_core::events::AgentEvent;

use crate::history::{HistoryEntry, HistoryStore, ReportMeta};
use crate::runner::{self, RunParams, RunUpdate};

const MAX_STATUS_LINES: usize = 200;
const PROVIDERS: [LlmProviderKind; 3] = [
    LlmProviderKind::Ollama,
    LlmProviderKind::Claude,
    LlmProviderKind::OpenAi,
];

pub struct ResearchApp {
    question_input: Entity<InputState>,
    provider_index: usize,
    max_iterations: u32,
    running: bool,
    status_lines: Vec<SharedString>,
    report: Option<SharedString>,
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
        let store = HistoryStore::open_default().expect("failed to open report store");
        let history = store.list();
        Self {
            question_input,
            provider_index: 0,
            max_iterations: 2,
            running: false,
            status_lines: Vec::new(),
            report: None,
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
        self.selected_stem = None;
        self.push_status(format!("調査開始: {question}"));

        let mut rx = runner::start(RunParams {
            question,
            provider: self.provider(),
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
            RunUpdate::Event(event) => self.push_status(describe_event(&event)),
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
                match self.store.save(meta, &outcome.markdown) {
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
            }
            Err(err) => self.push_status(format!("読み込みに失敗: {err:#}")),
        }
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
            }
        }
        cx.notify();
    }

    fn cycle_provider(&mut self, cx: &mut Context<Self>) {
        self.provider_index = (self.provider_index + 1) % PROVIDERS.len();
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
                    .on_click(cx.listener(|this, _, _, cx| this.cycle_provider(cx))),
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

    fn render_report(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let body: AnyElement = match &self.report {
            Some(markdown) => TextView::markdown("report-view", markdown.clone(), window, cx)
                .selectable(true)
                .into_any_element(),
            None => div()
                .text_color(cx.theme().muted_foreground)
                .child("レポートはまだありません。質問を入力して「調査開始」を押してください。")
                .into_any_element(),
        };
        div()
            .id("report-scroll")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .p_4()
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
            freshness,
            correctness,
            coverage,
            sufficient,
        } => format!(
            "自己評価: 鮮度 {freshness} / 正確性 {correctness} / 網羅性 {coverage}{}",
            if *sufficient {
                " — 十分と判定"
            } else {
                " — 追加調査へ"
            }
        ),
    }
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    match text.char_indices().nth(max_chars) {
        Some((index, _)) => format!("{}…", &text[..index]),
        None => text.to_string(),
    }
}
