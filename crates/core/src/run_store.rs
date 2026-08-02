//! Per-run output storage shared by frontends: each research run gets its
//! own directory `<base>/<YYYYMMDD>/<N>/` (N = attempt number within the
//! day) holding fixed-name files, so results are easy to browse by hand:
//!
//! ```text
//! data/
//!   20260715/
//!     1/
//!       report.md      final Markdown report
//!       meta.json      question, scores, counts, provider/model
//!       trace.jsonl    audit log of agent events (see [`crate::events`])
//!       run.log        detailed tracing output (written by the frontend)
//!   latest -> 20260715/1
//! ```

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AgentError, Result};

/// File names inside a run directory.
pub const REPORT_FILE: &str = "report.md";
pub const META_FILE: &str = "meta.json";
pub const TRACE_FILE: &str = "trace.jsonl";
pub const LOG_FILE: &str = "run.log";

/// Metadata saved as `meta.json`; records what was asked, how the agent
/// scored itself, and which LLM produced the report (for reproducibility).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunMeta {
    pub question: String,
    pub saved_at: String,
    pub provider: String,
    pub model: String,
    pub freshness: u8,
    pub correctness: u8,
    pub coverage: u8,
    pub finding_count: usize,
    pub source_count: usize,
    pub iterations: u32,
}

/// Directory-backed store rooted at a user-chosen base directory.
pub struct RunStore {
    base: PathBuf,
}

/// A freshly allocated run directory. Created at run start so the log and a
/// partial trace survive even when the run fails midway.
pub struct RunDir {
    path: PathBuf,
}

impl RunStore {
    pub fn open(base: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&base)?;
        Ok(Self { base })
    }

    pub fn base(&self) -> &Path {
        &self.base
    }

    /// Allocate the next `<base>/<YYYYMMDD>/<N>/` directory. `create_dir` is
    /// atomic (fails if the directory exists), so concurrent runs never share
    /// a directory: on collision we rescan and try the next number.
    pub fn create_run_dir(&self) -> Result<RunDir> {
        let day = chrono::Local::now().format("%Y%m%d").to_string();
        let day_dir = self.base.join(&day);
        std::fs::create_dir_all(&day_dir)?;

        for _ in 0..1000 {
            let seq = next_sequence(&day_dir)?;
            let path = day_dir.join(seq.to_string());
            match std::fs::create_dir(&path) {
                Ok(()) => {
                    self.update_latest_link(&day, seq);
                    return Ok(RunDir { path });
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(err) => return Err(err.into()),
            }
        }
        Err(AgentError::Config(format!(
            "could not allocate a run directory under {}",
            day_dir.display()
        )))
    }

    /// Point `<base>/latest` at the newest run. Best-effort: a run must not
    /// fail because a convenience symlink could not be written.
    #[cfg(unix)]
    fn update_latest_link(&self, day: &str, seq: u32) {
        let link = self.base.join("latest");
        let _ = std::fs::remove_file(&link);
        let _ = std::os::unix::fs::symlink(format!("{day}/{seq}"), &link);
    }

    #[cfg(not(unix))]
    fn update_latest_link(&self, _day: &str, _seq: u32) {}
}

/// Highest existing numeric directory name + 1 (1 when the day is empty).
/// Non-numeric entries are ignored so stray files never break allocation.
fn next_sequence(day_dir: &Path) -> Result<u32> {
    let mut max = 0u32;
    for entry in std::fs::read_dir(day_dir)? {
        let entry = entry?;
        if let Some(seq) = entry.file_name().to_str().and_then(|s| s.parse().ok()) {
            max = max.max(seq);
        }
    }
    Ok(max + 1)
}

impl RunDir {
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Where the frontend should write its detailed log for this run.
    pub fn log_path(&self) -> PathBuf {
        self.path.join(LOG_FILE)
    }

    /// Persist the final artifacts. The trace is written even when the
    /// report failed to materialize (empty markdown is skipped, an empty
    /// trace is skipped).
    pub fn save(&self, meta: &RunMeta, markdown: &str, trace_jsonl: &str) -> Result<()> {
        if !markdown.is_empty() {
            std::fs::write(self.path.join(REPORT_FILE), markdown)?;
        }
        std::fs::write(
            self.path.join(META_FILE),
            serde_json::to_string_pretty(meta)?,
        )?;
        if !trace_jsonl.is_empty() {
            std::fs::write(self.path.join(TRACE_FILE), trace_jsonl)?;
        }
        Ok(())
    }

    /// Persist only the trace (used when a run fails before producing a
    /// report, so the audit log is still available for debugging).
    pub fn save_trace(&self, trace_jsonl: &str) -> Result<()> {
        if !trace_jsonl.is_empty() {
            std::fs::write(self.path.join(TRACE_FILE), trace_jsonl)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> RunStore {
        let dir = std::env::temp_dir().join(format!(
            "agentic-search-run-store-test-{}-{:?}",
            std::process::id(),
            std::time::Instant::now()
        ));
        RunStore::open(dir).unwrap()
    }

    fn meta() -> RunMeta {
        RunMeta {
            question: "質問".into(),
            saved_at: "2026-07-15T00:00:00+09:00".into(),
            provider: "ollama".into(),
            model: "llama3.2:3b".into(),
            freshness: 80,
            correctness: 85,
            coverage: 90,
            finding_count: 10,
            source_count: 5,
            iterations: 2,
        }
    }

    #[test]
    fn allocates_sequential_run_dirs_per_day() {
        let store = temp_store();
        let first = store.create_run_dir().unwrap();
        let second = store.create_run_dir().unwrap();
        assert_eq!(first.path().file_name().unwrap(), "1");
        assert_eq!(second.path().file_name().unwrap(), "2");
        // Both live under the same YYYYMMDD directory.
        assert_eq!(first.path().parent(), second.path().parent());
        std::fs::remove_dir_all(store.base()).unwrap();
    }

    #[test]
    fn ignores_non_numeric_entries_when_numbering() {
        let store = temp_store();
        let first = store.create_run_dir().unwrap();
        let day_dir = first.path().parent().unwrap().to_path_buf();
        std::fs::create_dir(day_dir.join("notes")).unwrap();
        std::fs::write(day_dir.join("stray.txt"), "x").unwrap();
        let second = store.create_run_dir().unwrap();
        assert_eq!(second.path().file_name().unwrap(), "2");
        std::fs::remove_dir_all(store.base()).unwrap();
    }

    #[test]
    fn save_writes_fixed_file_names() {
        let store = temp_store();
        let run = store.create_run_dir().unwrap();
        let trace =
            r#"{"timestamp":"2026-07-15T00:00:00+09:00","type":"query_started","query":"q"}"#;
        run.save(&meta(), "# レポート", trace).unwrap();

        let report = std::fs::read_to_string(run.path().join(REPORT_FILE)).unwrap();
        assert_eq!(report, "# レポート");
        let loaded: RunMeta =
            serde_json::from_str(&std::fs::read_to_string(run.path().join(META_FILE)).unwrap())
                .unwrap();
        assert_eq!(loaded.question, "質問");
        assert_eq!(loaded.model, "llama3.2:3b");
        let loaded_trace = std::fs::read_to_string(run.path().join(TRACE_FILE)).unwrap();
        assert_eq!(loaded_trace, trace);
        std::fs::remove_dir_all(store.base()).unwrap();
    }

    #[test]
    fn empty_report_and_trace_are_not_written() {
        let store = temp_store();
        let run = store.create_run_dir().unwrap();
        run.save(&meta(), "", "").unwrap();
        assert!(!run.path().join(REPORT_FILE).exists());
        assert!(!run.path().join(TRACE_FILE).exists());
        assert!(run.path().join(META_FILE).exists());
        std::fs::remove_dir_all(store.base()).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn latest_symlink_tracks_newest_run() {
        let store = temp_store();
        store.create_run_dir().unwrap();
        let second = store.create_run_dir().unwrap();
        let resolved = std::fs::canonicalize(store.base().join("latest")).unwrap();
        assert_eq!(resolved, std::fs::canonicalize(second.path()).unwrap());
        std::fs::remove_dir_all(store.base()).unwrap();
    }
}
