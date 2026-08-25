//! Flux domain types.
//!
//! These mirror `packages/shared/src/types.ts` in the Flux repository. Fields are
//! deliberately permissive (`Option`, `#[serde(default)]`) because Heretic must
//! keep working against Flux servers that are older or newer than it is.

use serde::{Deserialize, Serialize};

/// Which tracker a project (and everything under it) comes from.
///
/// Stamped onto [`Project`] so the interface can route commands back to the
/// right source, and stored on each project binding. Flux is the default so
/// settings and API payloads written before this field existed keep meaning
/// what they meant.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    #[default]
    Flux,
    Linear,
}

impl SourceKind {
    pub fn label(self) -> &'static str {
        match self {
            SourceKind::Flux => "Flux",
            SourceKind::Linear => "Linear",
        }
    }
}

/// Where a task sits on the board.
///
/// Flux enforces one transition rule that Heretic must respect: a task in
/// [`TaskStatus::Planning`] cannot move straight to [`TaskStatus::InProgress`], it
/// has to pass through [`TaskStatus::Todo`] first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Planning,
    Todo,
    InProgress,
    Done,
}

impl TaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskStatus::Planning => "planning",
            TaskStatus::Todo => "todo",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::Done => "done",
        }
    }

    /// Whether Flux will accept a direct move from `self` to `next`.
    pub fn can_move_to(self, next: TaskStatus) -> bool {
        !(self == TaskStatus::Planning && next == TaskStatus::InProgress)
    }

    /// The status sequence needed to get from `self` to `target`, respecting
    /// Flux's planning -> todo -> in_progress rule.
    pub fn path_to(self, target: TaskStatus) -> Vec<TaskStatus> {
        if self == target {
            return Vec::new();
        }
        if self.can_move_to(target) {
            vec![target]
        } else {
            vec![TaskStatus::Todo, target]
        }
    }
}

/// P0 = urgent, P1 = normal, P2 = low.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub enum Priority {
    P0,
    P1,
    P2,
}

impl Priority {
    pub fn label(self) -> &'static str {
        match self {
            Priority::P0 => "P0",
            Priority::P1 => "P1",
            Priority::P2 => "P2",
        }
    }
}

impl TryFrom<u8> for Priority {
    type Error = String;
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Priority::P0),
            1 => Ok(Priority::P1),
            2 => Ok(Priority::P2),
            other => Err(format!("unknown Flux priority: {other}")),
        }
    }
}

impl From<Priority> for u8 {
    fn from(value: Priority) -> u8 {
        match value {
            Priority::P0 => 0,
            Priority::P1 => 1,
            Priority::P2 => 2,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub visibility: Option<String>,
    /// Which tracker this project lives on. Flux servers never send the field,
    /// so the default keeps their payloads meaning what they always meant.
    #[serde(default)]
    pub source: SourceKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Epic {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub notes: String,
    /// The gate Heretic honours: only epics with `auto = true` are worked
    /// autonomously. Everything else needs a human to press the button.
    #[serde(default)]
    pub auto: bool,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub project_id: String,
}

/// A numbered behavioural constraint. Higher number = more critical.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Guardrail {
    #[serde(default)]
    pub id: String,
    pub number: i64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskComment {
    #[serde(default)]
    pub id: String,
    pub body: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub agent_name: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub project_id: String,
    #[serde(default)]
    pub epic_id: Option<String>,
    /// Server-computed: true when a dependency is not yet done.
    #[serde(default)]
    pub blocked: bool,
    #[serde(default)]
    pub blocked_reason: Option<String>,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub priority: Option<Priority>,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
    #[serde(default)]
    pub guardrails: Vec<Guardrail>,
    #[serde(default)]
    pub comments: Vec<TaskComment>,
    #[serde(default)]
    pub blob_ids: Vec<String>,
    /// Agent names Flux believes are currently working this task. Maintained by
    /// the server when a status change is sent with an `agent_name`.
    #[serde(default)]
    pub workers: Vec<String>,
    /// Optional per-task agent hint set on the board (`claude` | `codex` | ...).
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

impl Task {
    pub fn status_enum(&self) -> Option<TaskStatus> {
        match self.status.as_str() {
            "planning" => Some(TaskStatus::Planning),
            "todo" => Some(TaskStatus::Todo),
            "in_progress" => Some(TaskStatus::InProgress),
            "done" => Some(TaskStatus::Done),
            _ => None,
        }
    }

    /// Priority for ordering, defaulting unset to P1 (normal) as Flux's board does.
    pub fn effective_priority(&self) -> Priority {
        self.priority.unwrap_or(Priority::P1)
    }

    /// Guardrails sorted most-critical first.
    pub fn guardrails_by_severity(&self) -> Vec<&Guardrail> {
        let mut sorted: Vec<&Guardrail> = self.guardrails.iter().collect();
        sorted.sort_by_key(|g| std::cmp::Reverse(g.number));
        sorted
    }
}
