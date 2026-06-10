mod evaluator;
mod gatherer;
mod knowledge;
mod planner;
mod prompts;
mod reporter;

use std::sync::Arc;

use chrono::Utc;
use tracing::info;

pub use evaluator::Evaluation;
pub use reporter::Report;

use crate::config::Limits;
use crate::error::Result;
use crate::events::{AgentEvent, EventSink};
use crate::fetch::PageFetcher;
use crate::llm::LlmClient;
use crate::search::SearchProvider;
use gatherer::Gatherer;
use knowledge::KnowledgeStore;

/// Character budget for the findings digest passed to evaluator/reporter,
/// sized for small local-model context windows.
const DIGEST_BUDGET: usize = 12_000;

/// The research agent: a plan -> gather -> self-evaluate loop that keeps
/// searching until its own reviewer judges the findings fresh, correct, and
/// complete (or the iteration budget runs out).
pub struct ResearchAgent {
    llm: Arc<dyn LlmClient>,
    search: Arc<dyn SearchProvider>,
    fetcher: Arc<dyn PageFetcher>,
    limits: Limits,
    events: Option<EventSink>,
    report_language: String,
}

impl ResearchAgent {
    pub fn new(
        llm: Arc<dyn LlmClient>,
        search: Arc<dyn SearchProvider>,
        fetcher: Arc<dyn PageFetcher>,
        limits: Limits,
    ) -> Self {
        Self {
            llm,
            search,
            fetcher,
            limits,
            events: None,
            report_language: "日本語".to_string(),
        }
    }

    /// Attach a progress-event callback (used by GUI frontends).
    pub fn with_events(mut self, sink: EventSink) -> Self {
        self.events = Some(sink);
        self
    }

    /// Override the language the final report is written in (default: 日本語).
    pub fn with_report_language(mut self, language: String) -> Self {
        self.report_language = language;
        self
    }

    fn emit(&self, event: AgentEvent) {
        if let Some(sink) = &self.events {
            sink(event);
        }
    }

    pub async fn run(&self, question: &str) -> Result<Report> {
        let today = Utc::now().format("%Y-%m-%d").to_string();
        let mut store = KnowledgeStore::new();

        let plan = planner::plan(self.llm.as_ref(), question, &today).await?;
        info!(sub_questions = ?plan.sub_questions, queries = ?plan.queries, "plan ready");
        self.emit(AgentEvent::PlanReady {
            queries: plan.queries.clone(),
        });

        let mut pending = plan.queries;
        let mut evaluation = Evaluation::default();
        let mut iteration = 0;
        while iteration < self.limits.max_iterations {
            iteration += 1;
            let added = self.run_iteration(question, &mut pending, &mut store).await;
            info!(
                iteration,
                new_findings = added,
                total = store.findings().len(),
                "gather done"
            );
            self.emit(AgentEvent::IterationDone {
                iteration,
                new_findings: added,
                total_findings: store.findings().len(),
            });

            evaluation = evaluator::evaluate(
                self.llm.as_ref(),
                question,
                &store.digest(DIGEST_BUDGET),
                &today,
            )
            .await?;
            info!(
                freshness = evaluation.freshness.score,
                correctness = evaluation.correctness.score,
                coverage = evaluation.coverage.score,
                sufficient = evaluation.sufficient(),
                "evaluation done"
            );
            self.emit(AgentEvent::EvaluationDone {
                iteration,
                evaluation: evaluation.clone(),
            });

            if evaluation.sufficient() {
                break;
            }
            pending = self.next_queries(&evaluation, &store);
            if pending.is_empty() && added == 0 {
                info!("no follow-up queries and no new findings; stopping early");
                break;
            }
        }

        reporter::write_report(reporter::ReportRequest {
            llm: self.llm.as_ref(),
            question,
            store: &store,
            evaluation,
            iterations: iteration,
            today: &today,
            digest_budget: DIGEST_BUDGET,
            language: &self.report_language,
        })
        .await
    }

    /// Run every pending query that has not been executed before. Per-query
    /// failures are logged inside the gatherer and do not abort the run.
    async fn run_iteration(
        &self,
        question: &str,
        pending: &mut Vec<String>,
        store: &mut KnowledgeStore,
    ) -> usize {
        let gatherer = Gatherer {
            llm: self.llm.as_ref(),
            search: self.search.as_ref(),
            fetcher: self.fetcher.as_ref(),
            limits: &self.limits,
            events: self.events.as_deref(),
        };
        let mut added = 0;
        for query in pending
            .drain(..)
            .take(self.limits.max_queries_per_iteration)
        {
            if !store.mark_query(&query) {
                continue;
            }
            self.emit(AgentEvent::QueryStarted {
                query: query.clone(),
            });
            match gatherer.gather(question, &query, store).await {
                Ok(count) => added += count,
                Err(err) => tracing::warn!(query, error = %err, "query failed"),
            }
        }
        added
    }

    /// Queries for the next iteration come from the evaluator's gap analysis;
    /// already-executed ones are dropped via `mark_query` at execution time,
    /// here we only cap the volume.
    fn next_queries(&self, evaluation: &Evaluation, _store: &KnowledgeStore) -> Vec<String> {
        evaluation
            .followup_queries
            .iter()
            .take(self.limits.max_queries_per_iteration)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;

    use crate::fetch::PageContent;
    use crate::llm::ChatRequest;
    use crate::search::SearchHit;

    /// Scripted LLM: routes by prompt role markers, counts evaluator calls.
    struct MockLlm {
        evaluator_calls: Mutex<u32>,
    }

    #[async_trait]
    impl LlmClient for MockLlm {
        async fn complete(&self, request: &ChatRequest) -> crate::error::Result<String> {
            if request.system.contains("research planner") {
                return Ok(r#"{"sub_questions": ["q1"], "queries": ["first query"]}"#.into());
            }
            if request.system.contains("extract facts") {
                return Ok(
                    r#"{"findings": [{"statement": "Mock fact", "published_hint": "2026-06-01"}]}"#
                        .into(),
                );
            }
            if request.system.contains("research reviewer") {
                let mut calls = self.evaluator_calls.lock().unwrap();
                *calls += 1;
                // Insufficient on the first pass, sufficient on the second.
                if *calls == 1 {
                    return Ok(r#"{
                        "freshness": {"score": 80, "issues": []},
                        "correctness": {"score": 80, "issues": []},
                        "coverage": {"score": 40, "issues": ["missing detail"]},
                        "is_sufficient": false,
                        "followup_queries": ["second query"]
                    }"#
                    .into());
                }
                return Ok(r#"{
                    "freshness": {"score": 85, "issues": []},
                    "correctness": {"score": 85, "issues": []},
                    "coverage": {"score": 90, "issues": []},
                    "is_sufficient": true,
                    "followup_queries": []
                }"#
                .into());
            }
            Ok("# Mock report".into())
        }
    }

    struct MockSearch;

    #[async_trait]
    impl crate::search::SearchProvider for MockSearch {
        async fn search(&self, query: &str, _limit: usize) -> crate::error::Result<Vec<SearchHit>> {
            Ok(vec![SearchHit {
                title: format!("Result for {query}"),
                url: format!("https://example.com/{}", query.replace(' ', "-")),
                snippet: "snippet".into(),
            }])
        }
    }

    struct MockFetcher;

    #[async_trait]
    impl PageFetcher for MockFetcher {
        async fn fetch(&self, url: &str) -> crate::error::Result<PageContent> {
            Ok(PageContent {
                url: url.to_string(),
                text: "Some page text".into(),
            })
        }
    }

    #[tokio::test]
    async fn loop_runs_followup_iteration_then_stops_when_sufficient() {
        let agent = ResearchAgent::new(
            Arc::new(MockLlm {
                evaluator_calls: Mutex::new(0),
            }),
            Arc::new(MockSearch),
            Arc::new(MockFetcher),
            Limits::default(),
        );
        let report = agent.run("test question").await.unwrap();
        assert_eq!(
            report.iterations, 2,
            "should iterate once more to fill the gap"
        );
        assert!(report.evaluation.sufficient());
        assert!(report.markdown.contains("Mock report"));
        assert!(report.markdown.contains("Self-assessment"));
        assert_eq!(report.finding_count, 1, "duplicate mock facts must dedupe");
    }
}
