use super::evaluator::Evaluation;
use super::knowledge::KnowledgeStore;
use super::prompts;
use crate::error::Result;
use crate::llm::{ChatRequest, LlmClient};

/// Final deliverable of a research run.
#[derive(Debug)]
pub struct Report {
    pub markdown: String,
    pub evaluation: Evaluation,
    pub finding_count: usize,
    pub source_count: usize,
    pub iterations: u32,
}

/// Inputs for the final report synthesis.
pub struct ReportRequest<'a> {
    pub llm: &'a dyn LlmClient,
    pub question: &'a str,
    pub store: &'a KnowledgeStore,
    pub evaluation: Evaluation,
    pub iterations: u32,
    pub today: &'a str,
    pub digest_budget: usize,
    pub language: &'a str,
}

/// Synthesize the final Markdown report from the knowledge store, appending
/// a transparency section with the agent's own quality assessment.
pub async fn write_report(request: ReportRequest<'_>) -> Result<Report> {
    let digest = request.store.digest(request.digest_budget);
    let chat = ChatRequest {
        system: prompts::reporter_system(request.language),
        user: prompts::reporter_user(request.question, &digest, request.today),
        json_mode: false,
    };
    let body = request.llm.complete(&chat).await?;
    let markdown = format!("{body}\n\n{}", quality_footer(&request.evaluation));
    Ok(Report {
        markdown,
        finding_count: request.store.findings().len(),
        source_count: request.store.source_count(),
        evaluation: request.evaluation,
        iterations: request.iterations,
    })
}

fn quality_footer(evaluation: &Evaluation) -> String {
    let mut footer = String::from("---\n\n## Self-assessment\n\n");
    footer.push_str(&format!(
        "| Axis | Score |\n|---|---|\n| Freshness | {} |\n| Correctness | {} |\n| Coverage | {} |\n",
        evaluation.freshness.score, evaluation.correctness.score, evaluation.coverage.score
    ));
    let issues: Vec<&String> = evaluation
        .freshness
        .issues
        .iter()
        .chain(&evaluation.correctness.issues)
        .chain(&evaluation.coverage.issues)
        .collect();
    if !issues.is_empty() {
        footer.push_str("\nKnown limitations:\n");
        for issue in issues {
            footer.push_str(&format!("- {issue}\n"));
        }
    }
    footer
}
