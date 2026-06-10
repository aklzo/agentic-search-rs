//! Progress events emitted by the agent so frontends can show live status.

/// One progress notification from a research run.
#[derive(Clone, Debug)]
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
    EvaluationDone {
        freshness: u8,
        correctness: u8,
        coverage: u8,
        sufficient: bool,
    },
}

/// Callback used to deliver events. Kept as a plain closure so the core
/// stays agnostic of the frontend's channel/executor choice.
pub type EventSink = Box<dyn Fn(AgentEvent) + Send + Sync>;
