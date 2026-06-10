//! Progress events emitted by the agent so frontends can show live status
//! and persist an audit trail of the run.

use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};

use crate::agent::Evaluation;

/// One progress notification from a research run. Serializable so frontends
/// can persist runs for later auditing.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    PlanReady {
        queries: Vec<String>,
    },
    QueryStarted {
        query: String,
    },
    PageProcessed {
        url: String,
        new_findings: usize,
    },
    IterationDone {
        iteration: u32,
        new_findings: usize,
        total_findings: usize,
    },
    /// Carries the full evaluation (scores, per-axis issues, follow-up
    /// queries) so audits can show *why* the agent kept searching.
    EvaluationDone {
        iteration: u32,
        evaluation: Evaluation,
    },
}

/// An event stamped with its occurrence time; one JSON line per record in
/// persisted trace files.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TraceRecord {
    pub timestamp: DateTime<Local>,
    #[serde(flatten)]
    pub event: AgentEvent,
}

impl TraceRecord {
    pub fn now(event: AgentEvent) -> Self {
        Self {
            timestamp: Local::now(),
            event,
        }
    }
}

/// Serialize records as JSON Lines (one JSON object per line).
pub fn to_jsonl(records: &[TraceRecord]) -> String {
    records
        .iter()
        .filter_map(|record| serde_json::to_string(record).ok())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Parse JSON Lines back into records, skipping unparseable lines so a
/// partially corrupted trace file still renders.
pub fn from_jsonl(text: &str) -> Vec<TraceRecord> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// Callback used to deliver events. Kept as a plain closure so the core
/// stays agnostic of the frontend's channel/executor choice.
pub type EventSink = Box<dyn Fn(AgentEvent) + Send + Sync>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_records_roundtrip_through_jsonl() {
        let records = vec![
            TraceRecord::now(AgentEvent::PlanReady {
                queries: vec!["q1".into(), "q2".into()],
            }),
            TraceRecord::now(AgentEvent::QueryStarted { query: "q1".into() }),
            TraceRecord::now(AgentEvent::EvaluationDone {
                iteration: 1,
                evaluation: Evaluation::default(),
            }),
        ];
        let jsonl = to_jsonl(&records);
        assert_eq!(jsonl.lines().count(), 3);
        assert!(jsonl.contains(r#""type":"plan_ready""#));

        let parsed = from_jsonl(&jsonl);
        assert_eq!(parsed.len(), 3);
        assert!(matches!(parsed[1].event, AgentEvent::QueryStarted { .. }));
    }

    #[test]
    fn from_jsonl_skips_corrupted_lines() {
        let jsonl = "not json\n{\"timestamp\":\"2026-06-11T00:00:00+09:00\",\"type\":\"query_started\",\"query\":\"q\"}\n";
        let parsed = from_jsonl(jsonl);
        assert_eq!(parsed.len(), 1);
    }
}
