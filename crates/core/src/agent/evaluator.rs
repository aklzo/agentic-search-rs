use serde::{Deserialize, Serialize};

use super::prompts;
use crate::error::Result;
use crate::llm::{ChatRequest, LlmClient};

/// Review of one quality axis (0-100 plus concrete issues).
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AspectReview {
    #[serde(default)]
    pub score: u8,
    #[serde(default)]
    pub issues: Vec<String>,
}

/// Self-evaluation of the collected knowledge: freshness (is it current?),
/// correctness (is it contradiction-free?), coverage (is it complete?).
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Evaluation {
    #[serde(default)]
    pub freshness: AspectReview,
    #[serde(default)]
    pub correctness: AspectReview,
    #[serde(default)]
    pub coverage: AspectReview,
    #[serde(default)]
    pub is_sufficient: bool,
    #[serde(default)]
    pub followup_queries: Vec<String>,
}

impl Evaluation {
    /// Guard against an over-optimistic judge: `is_sufficient` only counts
    /// when the per-axis scores back it up.
    pub fn sufficient(&self) -> bool {
        const THRESHOLD: u8 = 70;
        self.is_sufficient
            && self.freshness.score >= THRESHOLD
            && self.correctness.score >= THRESHOLD
            && self.coverage.score >= THRESHOLD
    }
}

/// Reflection step: have the LLM critique the current findings and propose
/// follow-up queries for whatever is missing, stale, or unverified.
pub async fn evaluate(
    llm: &dyn LlmClient,
    question: &str,
    digest: &str,
    today: &str,
) -> Result<Evaluation> {
    let request = ChatRequest {
        system: prompts::evaluator_system(),
        user: prompts::evaluator_user(question, digest, today),
        json_mode: true,
    };
    let value = llm.complete_json(&request).await?;
    Ok(serde_json::from_value(value)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn review(score: u8) -> AspectReview {
        AspectReview {
            score,
            issues: vec![],
        }
    }

    #[test]
    fn sufficiency_requires_flag_and_scores() {
        let mut eval = Evaluation {
            freshness: review(80),
            correctness: review(90),
            coverage: review(75),
            is_sufficient: true,
            followup_queries: vec![],
        };
        assert!(eval.sufficient());

        eval.coverage.score = 50;
        assert!(!eval.sufficient(), "low coverage must veto sufficiency");

        eval.coverage.score = 75;
        eval.is_sufficient = false;
        assert!(!eval.sufficient(), "judge verdict must be respected");
    }

    #[test]
    fn deserializes_partial_judge_output() {
        let value = serde_json::json!({
            "coverage": {"score": 40, "issues": ["missing pricing data"]},
            "followup_queries": ["product pricing 2026"]
        });
        let eval: Evaluation = serde_json::from_value(value).unwrap();
        assert_eq!(eval.coverage.score, 40);
        assert!(!eval.sufficient());
        assert_eq!(eval.followup_queries.len(), 1);
    }
}
