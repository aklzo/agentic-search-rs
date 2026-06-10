mod cli;

use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use tracing_subscriber::EnvFilter;

use agentic_search_core::agent::{self, ResearchAgent};
use agentic_search_core::config::{Config, LlmProviderKind};
use agentic_search_core::error;
use agentic_search_core::fetch::HttpFetcher;
use agentic_search_core::{llm, search};
use cli::Cli;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Cli::parse();
    init_logging(args.verbose);

    let config = build_config(&args).context("invalid configuration")?;
    let report = run_agent(&args, &config).await?;

    if let Some(path) = &args.output {
        std::fs::write(path, &report.markdown)
            .with_context(|| format!("failed to write {}", path.display()))?;
        eprintln!("report written to {}", path.display());
    } else {
        println!("{}", report.markdown);
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

fn init_logging(verbose: bool) {
    let default_level = if verbose {
        "agentic_search_core=debug,info"
    } else {
        "agentic_search_core=info"
    };
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_level));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
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

async fn run_agent(args: &Cli, config: &Config) -> anyhow::Result<agent::Report> {
    let llm = llm::build_client(&config.llm).context("failed to build LLM client")?;
    let search =
        search::build_provider(&config.search).context("failed to build search provider")?;
    let fetcher = Arc::new(HttpFetcher::new(&config.limits)?);

    let agent = ResearchAgent::new(llm, search, fetcher, config.limits.clone());
    agent
        .run(&args.question)
        .await
        .context("research run failed")
}
