use std::path::PathBuf;

use clap::Parser;

/// Agentic web research: plans searches, gathers sources, self-evaluates for
/// freshness/correctness/coverage, and keeps searching until satisfied.
#[derive(Debug, Parser)]
#[command(name = "agentic-search", version, about)]
pub struct Cli {
    /// Research question to investigate.
    pub question: String,

    /// LLM provider: ollama (default), claude, openai.
    #[arg(long)]
    pub provider: Option<String>,

    /// Model name override (e.g. "llama3.2:3b", "claude-sonnet-4-6").
    #[arg(long)]
    pub model: Option<String>,

    /// Maximum gather/evaluate iterations.
    #[arg(long)]
    pub max_iterations: Option<u32>,

    /// Write the Markdown report to this file instead of stdout only.
    #[arg(long)]
    pub output: Option<PathBuf>,

    /// Base directory for run outputs; each run is saved under
    /// <DIR>/<YYYYMMDD>/<N>/ (report.md, meta.json, trace.jsonl, run.log).
    /// Defaults to $AGS_DATA_DIR, then "./data".
    #[arg(long)]
    pub data_dir: Option<PathBuf>,

    /// Do not persist run outputs (report/meta/trace/log directory).
    #[arg(long)]
    pub no_save: bool,

    /// Verbose progress logging.
    #[arg(short, long)]
    pub verbose: bool,
}
