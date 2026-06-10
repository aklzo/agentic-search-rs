use serde::Deserialize;

use super::prompts;
use crate::error::Result;
use crate::llm::{ChatRequest, LlmClient};

/// Initial task decomposition produced by the planner LLM.
#[derive(Debug, Deserialize)]
pub struct Plan {
    #[serde(default)]
    pub sub_questions: Vec<String>,
    pub queries: Vec<String>,
}

/// Plan-and-execute step: turn the research question into sub-questions and
/// initial search queries.
pub async fn plan(llm: &dyn LlmClient, question: &str, today: &str) -> Result<Plan> {
    let request = ChatRequest {
        system: prompts::planner_system(),
        user: prompts::planner_user(question, today),
        json_mode: true,
    };
    let value = llm.complete_json(&request).await?;
    let mut plan: Plan = serde_json::from_value(value)?;
    if plan.queries.is_empty() {
        // Degenerate planner output: fall back to searching the question verbatim.
        plan.queries.push(question.to_string());
    }
    Ok(plan)
}
