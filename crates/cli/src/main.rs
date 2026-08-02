mod cli;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Context;
use clap::Parser;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;
use tracing_subscriber::{EnvFilter, Layer as _};

use agentic_search_core::agent::{self, ResearchAgent};
use agentic_search_core::config::{Config, LlmProviderKind};
use agentic_search_core::error;
use agentic_search_core::events::{self, TraceRecord};
use agentic_search_core::fetch::HttpFetcher;
use agentic_search_core::run_store::{RunDir, RunMeta, RunStore};
use agentic_search_core::{llm, search};
use cli::Cli;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Cli::parse();

    // Allocate the run directory before anything else so the log file and a
    // partial trace survive even when the run fails midway.
    let run_dir = open_run_dir(&args)?;
    init_logging(args.verbose, run_dir.as_ref().map(|dir| dir.log_path()))?;
    if let Some(dir) = &run_dir {
        eprintln!("run outputs: {}", dir.path().display());
    }

    let config = build_config(&args).context("invalid configuration")?;

    let trace: Arc<Mutex<Vec<TraceRecord>>> = Arc::default();
    let result = run_agent(&args, &config, Arc::clone(&trace)).await;
    let trace_jsonl = events::to_jsonl(&trace.lock().expect("trace lock poisoned"));

    let report = match result {
        Ok(report) => report,
        Err(err) => {
            // Keep the audit trail of the failed run for debugging.
            if let Some(dir) = &run_dir {
                if let Err(save_err) = dir.save_trace(&trace_jsonl) {
                    eprintln!("failed to save trace: {save_err}");
                }
            }
            return Err(err);
        }
    };

    if let Some(path) = &args.output {
        std::fs::write(path, &report.markdown)
            .with_context(|| format!("failed to write {}", path.display()))?;
        eprintln!("report written to {}", path.display());
    } else {
        println!("{}", report.markdown);
    }

    if let Some(dir) = &run_dir {
        let meta = RunMeta {
            question: args.question.clone(),
            saved_at: chrono::Local::now().to_rfc3339(),
            provider: config.llm.provider.as_str().to_string(),
            model: config.llm.model.clone(),
            freshness: report.evaluation.freshness.score,
            correctness: report.evaluation.correctness.score,
            coverage: report.evaluation.coverage.score,
            finding_count: report.finding_count,
            source_count: report.source_count,
            iterations: report.iterations,
        };
        dir.save(&meta, &report.markdown, &trace_jsonl)
            .with_context(|| format!("failed to save run to {}", dir.path().display()))?;
        eprintln!("run saved to {}", dir.path().display());
    }

    eprintln!(
        "done: {} findings from {} sources in {} iteration(s) | scores: freshness {}, correctness {}, coverage {}",
        report.finding_count,
        report.source_count,
        report.iterations,
        report.evaluation.freshness.score,
        report.evaluation.correctness.score,
        report.evaluation.coverage.score
    );
    Ok(())
}

/// Allocate `<data-dir>/<YYYYMMDD>/<N>/` unless saving is disabled.
/// Precedence for the base directory: `--data-dir` > `AGS_DATA_DIR` > `./data`.
fn open_run_dir(args: &Cli) -> anyhow::Result<Option<RunDir>> {
    if args.no_save {
        return Ok(None);
    }
    let base = args
        .data_dir
        .clone()
        .or_else(|| std::env::var("AGS_DATA_DIR").ok().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("data"));
    let store = RunStore::open(base).context("failed to open data directory")?;
    let dir = store
        .create_run_dir()
        .context("failed to allocate run directory")?;
    Ok(Some(dir))
}

/// Log to stderr (level controlled by -v / RUST_LOG) and, when the run is
/// being saved, mirror debug-level logs into run.log for later inspection.
fn init_logging(verbose: bool, log_file: Option<PathBuf>) -> anyhow::Result<()> {
    let default_level = if verbose {
        "agentic_search_core=debug,info"
    } else {
        "agentic_search_core=info"
    };
    let stderr_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));
    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_filter(stderr_filter);

    let file_layer = log_file
        .map(|path| -> anyhow::Result<_> {
            let file = std::fs::File::create(&path)
                .with_context(|| format!("failed to create {}", path.display()))?;
            Ok(tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(Arc::new(file))
                .with_filter(EnvFilter::new("agentic_search_core=debug,info")))
        })
        .transpose()?;

    tracing_subscriber::registry()
        .with(stderr_layer)
        .with(file_layer)
        .init();
    Ok(())
}

/// Environment configuration with CLI flags layered on top.
fn build_config(args: &Cli) -> error::Result<Config> {
    let provider = args
        .provider
        .as_deref()
        .map(LlmProviderKind::parse)
        .transpose()?;
    let mut config = Config::from_env(provider)?;
    if let Some(model) = &args.model {
        config.llm.model = model.clone();
    }
    if let Some(max_iterations) = args.max_iterations {
        config.limits.max_iterations = max_iterations;
    }
    Ok(config)
}

async fn run_agent(
    args: &Cli,
    config: &Config,
    trace: Arc<Mutex<Vec<TraceRecord>>>,
) -> anyhow::Result<agent::Report> {
    let llm = llm::build_client(&config.llm, config.limits.max_retries)
        .context("failed to build LLM client")?;
    let search =
        search::build_provider(&config.search).context("failed to build search provider")?;
    let fetcher = Arc::new(HttpFetcher::new(&config.limits)?);

    let agent = ResearchAgent::new(llm, search, fetcher, config.limits.clone())
        .with_report_language(config.report_language.clone())
        .with_events(Box::new(move |event| {
            eprintln!("{}", events::describe(&event));
            trace
                .lock()
                .expect("trace lock poisoned")
                .push(TraceRecord::now(event));
        }));
    agent
        .run(&args.question)
        .await
        .context("research run failed")
}
