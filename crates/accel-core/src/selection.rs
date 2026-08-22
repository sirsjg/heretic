//! Deciding what an agent is allowed to work on next.
//!
//! Autonomy in Accelerate is opt-in and narrow: a task is only eligible when the
//! user has switched `auto` on for its epic, the epic's own dependencies are
//! finished, and the task itself is genuinely actionable. Everything else needs a
//! human to start it explicitly.

use crate::model::{Epic, Priority, Task};
use std::collections::{HashMap, HashSet};

/// Why a task was not picked up. Surfaced in the UI so an idle queue is never a
/// mystery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ineligible {
    /// Not attached to any epic, so no `auto` flag governs it.
    NoEpic,
    /// The epic exists but is not switched to auto.
    EpicNotAuto,
    /// The epic is waiting on another epic that is not done.
    EpicBlockedByDependency,
    /// Not in `todo` — either still being planned, already running, or finished.
    WrongStatus,
    /// A dependency task is not done yet.
    BlockedByDependency,
    /// Someone recorded an external blocker on the task.
    ExternallyBlocked,
    Archived,
    /// Accelerate is already running this task.
    AlreadyRunning,
}

impl Ineligible {
    pub fn describe(self) -> &'static str {
        match self {
            Ineligible::NoEpic => "Not in an epic",
            Ineligible::EpicNotAuto => "Auto is off for this epic",
            Ineligible::EpicBlockedByDependency => "Epic is waiting on another epic",
            Ineligible::WrongStatus => "Not in Todo",
            Ineligible::BlockedByDependency => "Blocked by a dependency",
            Ineligible::ExternallyBlocked => "Blocked externally",
            Ineligible::Archived => "Archived",
            Ineligible::AlreadyRunning => "Already running",
        }
    }
}

/// A task that may be started, with the epic that authorised it.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub task: Task,
    pub epic_title: String,
}

/// Everything the selector needs to decide, gathered from Flux.
pub struct BoardSnapshot<'a> {
    pub epics: &'a [Epic],
    pub tasks: &'a [Task],
}

impl<'a> BoardSnapshot<'a> {
    /// Epics whose `auto` flag is on *and* whose epic-level dependencies are all done.
    fn runnable_epics(&self) -> HashMap<&'a str, &'a Epic> {
        let done: HashSet<&str> = self
            .epics
            .iter()
            .filter(|e| e.status == "done")
            .map(|e| e.id.as_str())
            .collect();

        self.epics
            .iter()
            .filter(|epic| epic.auto)
            .filter(|epic| {
                epic.depends_on
                    .iter()
                    .all(|dep| done.contains(dep.as_str()))
            })
            .map(|epic| (epic.id.as_str(), epic))
            .collect()
    }

    /// Task ids that are finished, used to resolve task-level dependencies
    /// ourselves rather than trusting only the server's `blocked` flag.
    fn done_tasks(&self) -> HashSet<&'a str> {
        self.tasks
            .iter()
            .filter(|t| t.status == "done")
            .map(|t| t.id.as_str())
            .collect()
    }

    /// Judge a single task. `running` holds task ids Accelerate already has in flight.
    pub fn eligibility(&self, task: &Task, running: &HashSet<String>) -> Option<Ineligible> {
        if task.archived {
            return Some(Ineligible::Archived);
        }
        if running.contains(&task.id) {
            return Some(Ineligible::AlreadyRunning);
        }

        let Some(epic_id) = task.epic_id.as_deref().filter(|id| !id.is_empty()) else {
            return Some(Ineligible::NoEpic);
        };

        let Some(epic) = self.epics.iter().find(|e| e.id == epic_id) else {
            return Some(Ineligible::NoEpic);
        };
        if !epic.auto {
            return Some(Ineligible::EpicNotAuto);
        }
        if !self.runnable_epics().contains_key(epic_id) {
            return Some(Ineligible::EpicBlockedByDependency);
        }

        if task.status != "todo" {
            return Some(Ineligible::WrongStatus);
        }
        if task
            .blocked_reason
            .as_deref()
            .is_some_and(|r| !r.trim().is_empty())
        {
            return Some(Ineligible::ExternallyBlocked);
        }

        let done = self.done_tasks();
        if task.blocked || !task.depends_on.iter().all(|d| done.contains(d.as_str())) {
            return Some(Ineligible::BlockedByDependency);
        }

        None
    }

    /// Every startable task, most important first.
    ///
    /// Ordering is P0 before P1 before P2, then oldest first within a priority so
    /// that started work finishes rather than being overtaken by newer arrivals.
    pub fn candidates(&self, running: &HashSet<String>) -> Vec<Candidate> {
        let runnable = self.runnable_epics();

        let mut candidates: Vec<Candidate> = self
            .tasks
            .iter()
            .filter(|task| self.eligibility(task, running).is_none())
            .map(|task| {
                let epic_title = task
                    .epic_id
                    .as_deref()
                    .and_then(|id| runnable.get(id))
                    .map(|epic| epic.title.clone())
                    .unwrap_or_default();
                Candidate {
                    task: task.clone(),
                    epic_title,
                }
            })
            .collect();

        candidates.sort_by(|a, b| {
            a.task
                .effective_priority()
                .cmp(&b.task.effective_priority())
                .then_with(|| created_key(&a.task).cmp(&created_key(&b.task)))
                .then_with(|| a.task.id.cmp(&b.task.id))
        });

        candidates
    }

    /// The next task to start, or `None` when the queue is dry.
    pub fn next_task(&self, running: &HashSet<String>) -> Option<Candidate> {
        self.candidates(running).into_iter().next()
    }
}

/// Sort key for creation time. Flux ids are random, so a missing timestamp sorts
/// last rather than pretending to be old.
fn created_key(task: &Task) -> (u8, String) {
    match task.created_at.as_deref().filter(|s| !s.is_empty()) {
        Some(ts) => (0, ts.to_string()),
        None => (1, String::new()),
    }
}

/// Count of tasks per priority among the given candidates, for the queue summary.
pub fn priority_breakdown(candidates: &[Candidate]) -> [usize; 3] {
    let mut counts = [0usize; 3];
    for candidate in candidates {
        let index = match candidate.task.effective_priority() {
            Priority::P0 => 0,
            Priority::P1 => 1,
            Priority::P2 => 2,
        };
        counts[index] += 1;
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Priority;

    fn epic(id: &str, auto: bool) -> Epic {
        Epic {
            id: id.into(),
            title: format!("Epic {id}"),
            status: "in_progress".into(),
            notes: String::new(),
            auto,
            depends_on: Vec::new(),
            project_id: "proj".into(),
        }
    }

    fn task(id: &str, epic_id: &str, status: &str) -> Task {
        Task {
            id: id.into(),
            title: format!("Task {id}"),
            status: status.into(),
            notes: None,
            depends_on: Vec::new(),
            project_id: "proj".into(),
            epic_id: Some(epic_id.into()),
            blocked: false,
            blocked_reason: None,
            archived: false,
            priority: Some(Priority::P1),
            acceptance_criteria: Vec::new(),
            guardrails: Vec::new(),
            comments: Vec::new(),
            blob_ids: Vec::new(),
            workers: Vec::new(),
            agent: None,
            created_at: Some("2026-01-01T00:00:00.000Z".into()),
            updated_at: None,
        }
    }

    fn none() -> HashSet<String> {
        HashSet::new()
    }

    #[test]
    fn only_tasks_in_auto_epics_are_eligible() {
        let epics = vec![epic("auto", true), epic("manual", false)];
        let tasks = vec![task("a", "auto", "todo"), task("b", "manual", "todo")];
        let board = BoardSnapshot {
            epics: &epics,
            tasks: &tasks,
        };

        let candidates = board.candidates(&none());
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].task.id, "a");
        assert_eq!(
            board.eligibility(&tasks[1], &none()),
            Some(Ineligible::EpicNotAuto)
        );
    }

    #[test]
    fn only_todo_tasks_are_started() {
        let epics = vec![epic("auto", true)];
        let tasks = vec![
            task("planning", "auto", "planning"),
            task("running", "auto", "in_progress"),
            task("finished", "auto", "done"),
            task("ready", "auto", "todo"),
        ];
        let board = BoardSnapshot {
            epics: &epics,
            tasks: &tasks,
        };

        let candidates = board.candidates(&none());
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].task.id, "ready");
    }

    #[test]
    fn epic_dependencies_gate_their_tasks() {
        let mut blocked_epic = epic("second", true);
        blocked_epic.depends_on = vec!["first".into()];
        let mut first = epic("first", true);
        first.status = "in_progress".into();

        let epics = vec![first, blocked_epic];
        let tasks = vec![task("t", "second", "todo")];
        let board = BoardSnapshot {
            epics: &epics,
            tasks: &tasks,
        };
        assert_eq!(
            board.eligibility(&tasks[0], &none()),
            Some(Ineligible::EpicBlockedByDependency)
        );

        // Once the prerequisite epic is done, the task unlocks.
        let epics = vec![
            Epic {
                status: "done".into(),
                ..epics[0].clone()
            },
            epics[1].clone(),
        ];
        let board = BoardSnapshot {
            epics: &epics,
            tasks: &tasks,
        };
        assert!(board.eligibility(&tasks[0], &none()).is_none());
    }

    #[test]
    fn task_dependencies_and_external_blockers_are_respected() {
        let epics = vec![epic("auto", true)];
        let mut dependent = task("b", "auto", "todo");
        dependent.depends_on = vec!["a".into()];
        let mut externally_blocked = task("c", "auto", "todo");
        externally_blocked.blocked_reason = Some("Waiting on vendor quote".into());

        let tasks = vec![
            task("a", "auto", "todo"),
            dependent.clone(),
            externally_blocked.clone(),
        ];
        let board = BoardSnapshot {
            epics: &epics,
            tasks: &tasks,
        };

        assert_eq!(
            board.eligibility(&dependent, &none()),
            Some(Ineligible::BlockedByDependency)
        );
        assert_eq!(
            board.eligibility(&externally_blocked, &none()),
            Some(Ineligible::ExternallyBlocked)
        );
        assert_eq!(board.candidates(&none()).len(), 1);
    }

    #[test]
    fn ordering_is_priority_then_oldest_first() {
        let epics = vec![epic("auto", true)];
        let mut urgent = task("urgent", "auto", "todo");
        urgent.priority = Some(Priority::P0);
        urgent.created_at = Some("2026-03-01T00:00:00.000Z".into());

        let mut old_normal = task("old", "auto", "todo");
        old_normal.created_at = Some("2026-01-01T00:00:00.000Z".into());

        let mut new_normal = task("new", "auto", "todo");
        new_normal.created_at = Some("2026-02-01T00:00:00.000Z".into());

        let mut low = task("low", "auto", "todo");
        low.priority = Some(Priority::P2);
        low.created_at = Some("2025-01-01T00:00:00.000Z".into());

        let tasks = vec![low, new_normal, urgent, old_normal];
        let board = BoardSnapshot {
            epics: &epics,
            tasks: &tasks,
        };

        let order: Vec<String> = board
            .candidates(&none())
            .into_iter()
            .map(|c| c.task.id)
            .collect();
        assert_eq!(order, vec!["urgent", "old", "new", "low"]);
    }

    #[test]
    fn running_tasks_are_not_started_twice() {
        let epics = vec![epic("auto", true)];
        let tasks = vec![task("a", "auto", "todo")];
        let board = BoardSnapshot {
            epics: &epics,
            tasks: &tasks,
        };

        let running: HashSet<String> = ["a".to_string()].into_iter().collect();
        assert_eq!(
            board.eligibility(&tasks[0], &running),
            Some(Ineligible::AlreadyRunning)
        );
        assert!(board.next_task(&running).is_none());
    }

    #[test]
    fn archived_tasks_never_run() {
        let epics = vec![epic("auto", true)];
        let mut archived = task("a", "auto", "todo");
        archived.archived = true;
        let tasks = vec![archived];
        let board = BoardSnapshot {
            epics: &epics,
            tasks: &tasks,
        };
        assert_eq!(
            board.eligibility(&tasks[0], &none()),
            Some(Ineligible::Archived)
        );
    }
}
