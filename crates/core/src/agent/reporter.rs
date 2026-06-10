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

/// Synthesize the final Markdown report from the knowledge store, appending
/// a transparency section with the agent's own quality assessment.
pub async fn write_report(
    llm: &dyn LlmClient,
    question: &str,
    store: &KnowledgeStore,
    evaluation: Evaluation,
    iterations: u32,
    today: &str,
    digest_budget: usize,
) -> Result<Report> {
    let digest = store.digest(digest_budget);
    let request = ChatRequest {
        system: prompts::reporter_system(),
        user: prompts::reporter_user(question, &digest, today),
        json_mode: false,
    };
    let body = llm.complete(&request).await?;
    let markdown = format!("{body}\n\n{}", quality_footer(&evaluation));
    Ok(Report {
        markdown,
        finding_count: store.findings().len(),
        source_count: store.source_count(),
        evaluation,
        iterations,
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
