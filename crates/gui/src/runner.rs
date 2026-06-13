//! Runs a research job on a dedicated thread with its own tokio runtime and
//! streams progress back to the GUI through a channel.

use std::sync::Arc;

use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use agentic_search_core::agent::ResearchAgent;
use agentic_search_core::config::{Config, LlmProviderKind};
use agentic_search_core::events::AgentEvent;
use agentic_search_core::fetch::HttpFetcher;
use agentic_search_core::{llm, search};

/// Final result of a successful run, ready for display and saving.
#[derive(Debug)]
pub struct RunOutcome {
    pub question: String,
    pub markdown: String,
    pub freshness: u8,
    pub correctness: u8,
    pub coverage: u8,
    pub finding_count: usize,
    pub source_count: usize,
    pub iterations: u32,
}

/// Messages delivered to the GUI while a run is in flight.
#[derive(Debug)]
pub enum RunUpdate {
    Event(AgentEvent),
    Finished(Box<RunOutcome>),
    Failed(String),
}

/// Job parameters captured from the UI controls.
#[derive(Clone, Debug)]
pub struct RunParams {
    pub question: String,
    pub provider: LlmProviderKind,
    /// Model name selected in the UI; `None` falls back to the provider's
    /// configured default.
    pub model: Option<String>,
    pub max_iterations: u32,
}

/// Spawn the research job; the returned receiver yields progress updates and
/// ends with either `Finished` or `Failed`.
pub fn start(params: RunParams) -> UnboundedReceiver<RunUpdate> {
    let (tx, rx) = unbounded_channel();
    std::thread::Builder::new()
        .name("research-runner".into())
        .spawn(move || {
            if let Err(err) = run_blocking(params, &tx) {
                let _ = tx.send(RunUpdate::Failed(format!("{err:#}")));
            }
        })
        .expect("failed to spawn runner thread");
    rx
}

/// Fetch the models installed on the local Ollama server; delivers one list
/// (empty when the server is unreachable) and closes the channel.
pub fn fetch_ollama_models() -> UnboundedReceiver<Vec<String>> {
    let (tx, rx) = unbounded_channel();
    std::thread::Builder::new()
        .name("ollama-models".into())
        .spawn(move || {
            let base_url = std::env::var("AGS_LLM_BASE_URL").unwrap_or_else(|_| {
                agentic_search_core::config::default_base_url(LlmProviderKind::Ollama).to_string()
            });
            let models = tokio::runtime::Runtime::new()
                .ok()
                .and_then(|runtime| runtime.block_on(llm::list_ollama_models(&base_url)).ok())
                .unwrap_or_default();
            let _ = tx.send(models);
        })
        .expect("failed to spawn ollama-models thread");
    rx
}

fn run_blocking(params: RunParams, tx: &UnboundedSender<RunUpdate>) -> anyhow::Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        let mut config = Config::from_env(Some(params.provider))?;
        config.limits.max_iterations = params.max_iterations;
        if let Some(model) = params.model {
            config.llm.model = model;
        }

        let llm = llm::build_client(&config.llm, config.limits.max_retries)?;
        let search = search::build_provider(&config.search)?;
        let fetcher = Arc::new(HttpFetcher::new(&config.limits)?);

        let events_tx = tx.clone();
        let agent = ResearchAgent::new(llm, search, fetcher, config.limits.clone())
            .with_report_language(config.report_language.clone())
            .with_events(Box::new(move |event| {
                let _ = events_tx.send(RunUpdate::Event(event));
            }));

        let report = agent.run(&params.question).await?;
        let outcome = RunOutcome {
            question: params.question,
            markdown: report.markdown,
            freshness: report.evaluation.freshness.score,
            correctness: report.evaluation.correctness.score,
            coverage: report.evaluation.coverage.score,
            finding_count: report.finding_count,
            source_count: report.source_count,
            iterations: report.iterations,
        };
        let _ = tx.send(RunUpdate::Finished(Box::new(outcome)));
        Ok(())
    })
}
