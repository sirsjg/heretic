//! Run history on disk.
//!
//! Settings are small and rewritten whole. A run is neither: it arrives an
//! event at a time, for as long as the agents work. So each run gets its own
//! append-only JSONL journal — a snapshot line whenever the record changes, a
//! feed line for every event as it happens — and reading one back means taking
//! the last snapshot and replaying the feed onto it.
//!
//! Append-only is the point. Nothing rewrites a journal while a run is in
//! flight, so a crash costs at most the line being written, and the transcript
//! kept here is the full one rather than the last [`RunRecord::FEED_LIMIT`]
//! events the UI holds in memory.
//!
//! Nothing in this module is allowed to fail loudly. History is a convenience;
//! a disk that will not take it must never stop a run.

use crate::orchestrator::{RunFeedItem, RunRecord};
use crate::paths;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// One line of a journal.
///
/// Tagged rather than positional so a reader can skip what it does not
/// recognise and keep the rest of the run.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "line", rename_all = "snake_case")]
enum Entry {
    /// The run as it stood. The last one in the file wins.
    ///
    /// Boxed to keep this enum the size of its smaller variant; the feed line
    /// is written thousands of times more often.
    Record { run: Box<RunRecord> },
    /// One activity-feed item, written as it arrived.
    Feed { item: Box<RunFeedItem> },
}

/// Reads and writes run journals.
#[derive(Debug)]
pub struct RunHistory {
    /// Where journals live. `None` keeps history entirely in memory, which is
    /// what tests and [`Engine::new`](crate::Engine::new) want.
    dir: Option<PathBuf>,
    /// How many runs to keep on disk.
    keep: usize,
    /// Lines written per run this session, so one runaway agent cannot fill the
    /// disk with a single journal, and a journal that will not write is only
    /// complained about once.
    written: Mutex<HashMap<String, usize>>,
}

impl Default for RunHistory {
    fn default() -> Self {
        Self::new(paths::runs_dir())
    }
}

impl RunHistory {
    /// How many runs survive a restart. Older journals are deleted on load.
    pub const KEEP: usize = 200;

    /// Ceiling on one journal's length. A hundred thousand lines of transcript
    /// is already far past what anyone will read; past that the run keeps
    /// working and stops being written down.
    const MAX_LINES: usize = 100_000;

    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: Some(dir.into()),
            keep: Self::KEEP,
            written: Mutex::new(HashMap::new()),
        }
    }

    /// History that goes nowhere — runs live and die with the process.
    pub fn disabled() -> Self {
        Self {
            dir: None,
            keep: Self::KEEP,
            written: Mutex::new(HashMap::new()),
        }
    }

    /// Keep a different number of runs than [`Self::KEEP`].
    pub fn keeping(mut self, keep: usize) -> Self {
        self.keep = keep;
        self
    }

    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    /// Every run on disk, newest first, with the oldest pruned away.
    ///
    /// A journal that cannot be read is skipped rather than fatal: one corrupt
    /// file should cost its own run, not the whole history.
    pub fn load(&self) -> Vec<RunRecord> {
        let Some(dir) = self.dir.as_deref() else {
            return Vec::new();
        };

        let listing = match std::fs::read_dir(dir) {
            Ok(listing) => listing,
            // Nothing has ever been written. That is the first run, not a fault.
            Err(error) if error.kind() == ErrorKind::NotFound => return Vec::new(),
            Err(error) => {
                tracing::warn!(%error, dir = %dir.display(), "could not read run history");
                return Vec::new();
            }
        };

        let mut runs: Vec<RunRecord> = listing
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
            .filter_map(|path| read_journal(&path))
            .collect();

        runs.sort_by(|a, b| b.started_at.cmp(&a.started_at));

        for stale in runs.split_off(runs.len().min(self.keep)) {
            self.forget(&stale.id);
        }

        runs
    }

    /// Write down how the run stands now.
    ///
    /// The feed is left out: it is already on disk, an event at a time, and
    /// repeating it in every snapshot would grow the journal quadratically.
    pub fn record(&self, run: &RunRecord) {
        if self.dir.is_none() {
            return;
        }
        let snapshot = RunRecord {
            feed: Vec::new(),
            ..run.clone()
        };
        self.append(
            &run.id,
            &Entry::Record {
                run: Box::new(snapshot),
            },
        );
    }

    /// Write down one feed item.
    pub fn feed(&self, run_id: &str, item: &RunFeedItem) {
        if self.dir.is_none() {
            return;
        }
        self.append(
            run_id,
            &Entry::Feed {
                item: Box::new(item.clone()),
            },
        );
    }

    /// Delete a run's journal — what dismissing a run from the list means here.
    pub fn forget(&self, run_id: &str) {
        let Some(path) = self.journal_path(run_id) else {
            return;
        };

        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                tracing::warn!(%error, path = %path.display(), "could not delete run history")
            }
        }

        self.written
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(run_id);
    }

    /// Append one line, holding the lock across the write so lines from
    /// different threads cannot interleave within a journal.
    fn append(&self, run_id: &str, entry: &Entry) {
        let Some(path) = self.journal_path(run_id) else {
            return;
        };

        let mut written = self
            .written
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let count = written.entry(run_id.to_string()).or_default();
        if *count >= Self::MAX_LINES {
            return;
        }

        if let Err(error) = write_line(&path, entry) {
            tracing::warn!(%error, path = %path.display(), "could not write run history");
            // Stop writing this run rather than logging once per event.
            *count = Self::MAX_LINES;
            return;
        }

        *count += 1;
    }

    /// Run ids are UUIDs. Anything else is refused rather than trusted to be a
    /// safe file name.
    fn journal_path(&self, run_id: &str) -> Option<PathBuf> {
        let dir = self.dir.as_deref()?;
        let safe = !run_id.is_empty()
            && run_id.len() <= 64
            && run_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        safe.then(|| dir.join(format!("{run_id}.jsonl")))
    }
}

fn write_line(path: &Path, entry: &Entry) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut line = serde_json::to_string(entry).map_err(std::io::Error::other)?;
    line.push('\n');

    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(line.as_bytes())
}

/// Replay one journal: the last snapshot, with the feed laid back on top.
fn read_journal(path: &Path) -> Option<RunRecord> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) => {
            tracing::warn!(%error, path = %path.display(), "could not open run history");
            return None;
        }
    };

    let mut record: Option<RunRecord> = None;
    let mut feed: Vec<RunFeedItem> = Vec::new();

    for line in BufReader::new(file).lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Entry>(&line) {
            Ok(Entry::Record { run }) => record = Some(*run),
            Ok(Entry::Feed { item }) => feed.push(*item),
            // A line from a build whose shape has since changed, or one torn in
            // half by a crash mid-write. Costs an event, not the run.
            Err(_) => continue,
        }
    }

    let mut record = record?;
    if feed.len() > RunRecord::FEED_LIMIT {
        feed.drain(..feed.len() - RunRecord::FEED_LIMIT);
    }
    record.feed = feed;
    Some(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator::{Landing, RunStage, RunStatus};
    use crate::runner::AgentEvent;
    use crate::worktree::ChangeSummary;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "heretic-history-{name}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&path);
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn a_run(id: &str, started_at: &str) -> RunRecord {
        RunRecord {
            id: id.into(),
            project_id: "proj1".into(),
            project_name: "Heretic".into(),
            task_id: "task1".into(),
            task_title: "Persist run history".into(),
            epic_title: "Storage".into(),
            status: RunStatus::Running,
            stage: RunStage::Implementing,
            agent: Some("Claude Code · Implementer".into()),
            started_at: started_at.into(),
            finished_at: None,
            revisions: 0,
            branch: Some("heretic/task1".into()),
            base_branch: Some("main".into()),
            worktree_path: Some("/tmp/worktree".into()),
            landing: Landing::Nothing,
            changes: ChangeSummary::default(),
            result: None,
            stats: Vec::new(),
            feed: Vec::new(),
        }
    }

    fn an_item(text: &str) -> RunFeedItem {
        RunFeedItem {
            stage: RunStage::Implementing,
            role: None,
            event: AgentEvent::Text { text: text.into() },
        }
    }

    #[test]
    fn a_run_survives_a_round_trip_with_its_feed() {
        let dir = TempDir::new("roundtrip");
        let history = RunHistory::new(&dir.0);

        let run = a_run("run-1", "2026-08-24T10:00:00Z");
        history.record(&run);
        history.feed(&run.id, &an_item("first"));
        history.feed(&run.id, &an_item("second"));

        let loaded = history.load();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "run-1");
        assert_eq!(loaded[0].task_title, "Persist run history");
        assert_eq!(loaded[0].branch.as_deref(), Some("heretic/task1"));
        assert_eq!(loaded[0].feed, vec![an_item("first"), an_item("second")]);
    }

    #[test]
    fn the_last_snapshot_wins() {
        let dir = TempDir::new("last-wins");
        let history = RunHistory::new(&dir.0);

        let mut run = a_run("run-1", "2026-08-24T10:00:00Z");
        history.record(&run);
        history.feed(&run.id, &an_item("working"));
        run.status = RunStatus::Succeeded;
        run.landing = Landing::Merged;
        history.record(&run);

        let loaded = history.load();
        assert_eq!(loaded[0].status, RunStatus::Succeeded);
        assert_eq!(loaded[0].landing, Landing::Merged);
        // The feed is written once and replayed, not repeated per snapshot.
        assert_eq!(loaded[0].feed, vec![an_item("working")]);
    }

    #[test]
    fn a_long_transcript_is_kept_on_disk_but_trimmed_on_the_way_in() {
        let dir = TempDir::new("trim");
        let history = RunHistory::new(&dir.0);

        let run = a_run("run-1", "2026-08-24T10:00:00Z");
        history.record(&run);
        let total = RunRecord::FEED_LIMIT + 10;
        for index in 0..total {
            history.feed(&run.id, &an_item(&format!("event {index}")));
        }

        let journal = std::fs::read_to_string(dir.0.join("run-1.jsonl")).unwrap();
        assert_eq!(journal.lines().count(), total + 1);

        let loaded = history.load();
        assert_eq!(loaded[0].feed.len(), RunRecord::FEED_LIMIT);
        // Trimmed from the front: the newest events are the ones kept.
        assert_eq!(
            loaded[0].feed.last(),
            Some(&an_item(&format!("event {}", total - 1)))
        );
    }

    #[test]
    fn an_unreadable_line_costs_one_event_not_the_run() {
        let dir = TempDir::new("torn");
        let history = RunHistory::new(&dir.0);

        let run = a_run("run-1", "2026-08-24T10:00:00Z");
        history.record(&run);
        history.feed(&run.id, &an_item("before"));

        let path = dir.0.join("run-1.jsonl");
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(b"{\"line\":\"feed\",\"item\":{\"stage\":\"imple\n")
            .unwrap();
        drop(file);

        history.feed(&run.id, &an_item("after"));

        let loaded = history.load();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].feed, vec![an_item("before"), an_item("after")]);
    }

    #[test]
    fn a_record_from_an_older_build_still_loads() {
        let dir = TempDir::new("older");
        std::fs::create_dir_all(&dir.0).unwrap();
        // Everything past the identifying fields has since been added.
        std::fs::write(
            dir.0.join("run-1.jsonl"),
            r#"{"line":"record","run":{"id":"run-1","project_id":"proj1","task_id":"task1","status":"succeeded","stage":"integrating","started_at":"2026-08-24T10:00:00Z"}}
"#,
        )
        .unwrap();

        let loaded = RunHistory::new(&dir.0).load();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "run-1");
        assert_eq!(loaded[0].landing, Landing::Nothing);
        assert!(loaded[0].stats.is_empty());
    }

    #[test]
    fn forgetting_a_run_deletes_its_journal() {
        let dir = TempDir::new("forget");
        let history = RunHistory::new(&dir.0);

        history.record(&a_run("run-1", "2026-08-24T10:00:00Z"));
        history.record(&a_run("run-2", "2026-08-24T11:00:00Z"));
        history.forget("run-1");

        let loaded = history.load();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "run-2");
        assert!(!dir.0.join("run-1.jsonl").exists());
    }

    #[test]
    fn runs_come_back_newest_first_and_the_oldest_are_pruned() {
        let dir = TempDir::new("prune");
        let history = RunHistory::new(&dir.0).keeping(2);

        for hour in 9..13 {
            history.record(&a_run(
                &format!("run-{hour}"),
                &format!("2026-08-24T{hour:02}:00:00Z"),
            ));
        }

        let loaded = history.load();
        let ids: Vec<&str> = loaded.iter().map(|run| run.id.as_str()).collect();
        assert_eq!(ids, ["run-12", "run-11"]);
        // Pruning is a deletion, so it holds across the next load too.
        assert!(!dir.0.join("run-9.jsonl").exists());
        assert_eq!(history.load().len(), 2);
    }

    #[test]
    fn a_run_id_that_is_not_a_safe_file_name_is_refused() {
        let dir = TempDir::new("unsafe-id");
        let history = RunHistory::new(&dir.0);

        history.feed("../../settings", &an_item("nope"));

        assert!(history.journal_path("../../settings").is_none());
        assert!(history.load().is_empty());
    }

    #[test]
    fn disabled_history_writes_nothing_and_loads_nothing() {
        let history = RunHistory::disabled();
        history.record(&a_run("run-1", "2026-08-24T10:00:00Z"));
        history.feed("run-1", &an_item("ignored"));
        assert!(history.load().is_empty());
        assert!(history.dir().is_none());
    }
}
