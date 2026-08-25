//! Translating Linear's shapes into the canonical model.
//!
//! The vocabulary shifts one level: a Linear *team* holds the issues and the
//! workflow, so it becomes a Heretic project; a Linear *project* groups issues
//! toward an outcome, so it becomes an epic; an issue is a task. Everything
//! downstream of this file speaks only the canonical model.

use crate::model::{Epic, Guardrail, Priority, Project, SourceKind, Task, TaskComment, TaskStatus};
use serde::Deserialize;

// --- Raw GraphQL shapes -------------------------------------------------------
//
// Fields are permissive (`Option`, `default`) for the same reason the canonical
// model's are: a Linear API change should degrade a field, not lose the board.

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Nodes<T> {
    #[serde(default = "Vec::new")]
    pub nodes: Vec<T>,
}

impl<T> Default for Nodes<T> {
    fn default() -> Self {
        Self { nodes: Vec::new() }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PageInfo {
    #[serde(default)]
    pub has_next_page: bool,
    #[serde(default)]
    pub end_cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Page<T> {
    #[serde(default = "Vec::new")]
    pub nodes: Vec<T>,
    #[serde(default = "default_page_info")]
    pub page_info: PageInfo,
}

fn default_page_info() -> PageInfo {
    PageInfo {
        has_next_page: false,
        end_cursor: None,
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawTeam {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

/// A Linear project — Heretic's epic.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawEpic {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub teams: Option<Nodes<IdOnly>>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct IdOnly {
    pub id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawWorkflowState {
    pub id: String,
    #[serde(rename = "type")]
    pub state_type: String,
    #[serde(default)]
    pub position: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawStateRef {
    #[serde(rename = "type", default)]
    pub state_type: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawUser {
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawComment {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub user: Option<RawUser>,
    #[serde(default)]
    pub bot_actor: Option<RawUser>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawRelation {
    #[serde(rename = "type", default)]
    pub relation_type: String,
    /// On an inverse relation this is the *other* issue — the one doing the
    /// blocking.
    #[serde(default)]
    pub issue: Option<IdOnly>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RawIssue {
    pub id: String,
    #[serde(default)]
    pub identifier: Option<String>,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    /// Linear's scale: 0 none, 1 urgent, 2 high, 3 medium, 4 low.
    #[serde(default)]
    pub priority: Option<i64>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub archived_at: Option<String>,
    #[serde(default)]
    pub state: Option<RawStateRef>,
    #[serde(default)]
    pub team: Option<IdOnly>,
    #[serde(default)]
    pub project: Option<IdOnly>,
    #[serde(default)]
    pub comments: Nodes<RawComment>,
    /// Relations where this issue is on the receiving end; `type == "blocks"`
    /// here means the related issue blocks this one.
    #[serde(default)]
    pub inverse_relations: Nodes<RawRelation>,
}

// --- Status vocabulary --------------------------------------------------------

/// Linear workflow-state *types* onto board columns. Teams rename and multiply
/// states freely, but every state carries one of these fixed types, so the
/// mapping survives any amount of workflow customisation.
pub(crate) fn status_from_state_type(state_type: &str) -> &'static str {
    match state_type {
        "triage" | "backlog" => "planning",
        "unstarted" => "todo",
        "started" => "in_progress",
        "completed" => "done",
        // Cancelled work is off the board, not done — a dependency on it must
        // not be treated as satisfied.
        "canceled" => "canceled",
        _ => "todo",
    }
}

/// The state types that can represent a board column, most specific first.
/// A team is not obliged to have all six types, so movement falls back.
pub(crate) fn state_types_for(status: TaskStatus) -> &'static [&'static str] {
    match status {
        TaskStatus::Planning => &["backlog", "triage"],
        TaskStatus::Todo => &["unstarted", "backlog"],
        TaskStatus::InProgress => &["started"],
        TaskStatus::Done => &["completed"],
    }
}

/// The workflow state a move should land on: the first preferred type the team
/// actually has, and within it the leftmost state on the board.
pub(crate) fn pick_state(
    states: &[RawWorkflowState],
    status: TaskStatus,
) -> Option<&RawWorkflowState> {
    for wanted in state_types_for(status) {
        let candidate = states
            .iter()
            .filter(|s| s.state_type == *wanted)
            .min_by(|a, b| a.position.total_cmp(&b.position));
        if candidate.is_some() {
            return candidate;
        }
    }
    None
}

fn priority_from_linear(priority: Option<i64>) -> Option<Priority> {
    match priority {
        Some(1) => Some(Priority::P0),
        Some(2) | Some(3) => Some(Priority::P1),
        Some(4) => Some(Priority::P2),
        // 0 is "no priority"; the canonical model's default (P1) applies.
        _ => None,
    }
}

// --- Conversions --------------------------------------------------------------

pub(crate) fn project_from_team(team: RawTeam) -> Project {
    let name = match team.key.as_deref().filter(|k| !k.is_empty()) {
        Some(key) => format!("{} ({key})", team.name),
        None => team.name,
    };
    Project {
        id: team.id,
        name,
        description: team.description,
        visibility: None,
        source: SourceKind::Linear,
    }
}

/// `auto` is Heretic's own switch: Linear has no equivalent field, so the flag
/// lives in settings and is stamped on here.
pub(crate) fn epic_from_project(epic: RawEpic, team_id: &str, auto: bool) -> Epic {
    let status = match epic.state.as_deref() {
        Some("completed") => "done".to_string(),
        Some(other) => other.to_string(),
        None => String::new(),
    };
    Epic {
        id: epic.id,
        title: epic.name,
        status,
        notes: epic.description.unwrap_or_default(),
        auto,
        depends_on: Vec::new(),
        project_id: team_id.to_string(),
    }
}

pub(crate) fn task_from_issue(issue: RawIssue, team_id: &str) -> Task {
    let status = issue
        .state
        .as_ref()
        .and_then(|s| s.state_type.as_deref())
        .map(status_from_state_type)
        .unwrap_or("todo")
        .to_string();

    let sections = parse_description(issue.description.as_deref().unwrap_or(""));

    let depends_on = issue
        .inverse_relations
        .nodes
        .iter()
        .filter(|r| r.relation_type == "blocks")
        .filter_map(|r| r.issue.as_ref().map(|i| i.id.clone()))
        .collect();

    let comments = issue
        .comments
        .nodes
        .into_iter()
        .map(|c| {
            let author = c
                .user
                .as_ref()
                .and_then(|u| u.display_name.clone().or_else(|| u.name.clone()))
                .or_else(|| c.bot_actor.as_ref().and_then(|b| b.name.clone()));
            TaskComment {
                id: c.id.unwrap_or_default(),
                body: c.body,
                author,
                agent_name: None,
                created_at: c.created_at,
            }
        })
        .collect();

    let title = match issue.identifier.as_deref().filter(|i| !i.is_empty()) {
        Some(identifier) => format!("{identifier} — {}", issue.title),
        None => issue.title,
    };

    Task {
        id: issue.id,
        title,
        status,
        notes: (!sections.notes.is_empty()).then_some(sections.notes),
        depends_on,
        project_id: team_id.to_string(),
        epic_id: issue.project.map(|p| p.id),
        // Left for the selector, which resolves dependencies against the board
        // it fetched rather than trusting a server-computed flag.
        blocked: false,
        blocked_reason: None,
        archived: issue.archived_at.is_some(),
        priority: priority_from_linear(issue.priority),
        acceptance_criteria: sections.acceptance_criteria,
        guardrails: sections.guardrails,
        comments,
        blob_ids: Vec::new(),
        workers: Vec::new(),
        agent: None,
        created_at: issue.created_at,
        updated_at: issue.updated_at,
    }
}

// --- Description sections -----------------------------------------------------

/// What a parsed issue description yields: the prose, and any structured
/// sections Heretic knows how to honour.
#[derive(Debug, Default)]
pub(crate) struct DescriptionSections {
    pub notes: String,
    pub acceptance_criteria: Vec<String>,
    pub guardrails: Vec<Guardrail>,
}

#[derive(PartialEq)]
enum Section {
    Notes,
    Acceptance,
    Guardrails,
}

/// Lift `## Acceptance criteria` and `## Guardrails` sections out of an issue
/// description, by convention, since Linear has no native fields for either.
/// The lifted items land in the same prompt slots a Flux task's would; the
/// rest of the description stays prose.
pub(crate) fn parse_description(description: &str) -> DescriptionSections {
    let mut notes: Vec<&str> = Vec::new();
    let mut acceptance: Vec<String> = Vec::new();
    let mut guardrail_texts: Vec<String> = Vec::new();
    let mut current = Section::Notes;

    for line in description.lines() {
        if let Some(title) = heading_title(line) {
            let lowered = title.to_lowercase();
            current = if lowered.contains("acceptance criteria") {
                Section::Acceptance
            } else if lowered.contains("guardrail") {
                Section::Guardrails
            } else {
                // Any other heading returns to prose, heading included.
                notes.push(line);
                Section::Notes
            };
            continue;
        }

        match current {
            Section::Notes => notes.push(line),
            Section::Acceptance => {
                if let Some(item) = list_item(line) {
                    acceptance.push(item.to_string());
                } else if !line.trim().is_empty() {
                    notes.push(line);
                }
            }
            Section::Guardrails => {
                if let Some(item) = list_item(line) {
                    guardrail_texts.push(item.to_string());
                } else if !line.trim().is_empty() {
                    notes.push(line);
                }
            }
        }
    }

    // Flux ranks guardrails by number, higher = more critical, and the prompt
    // presents them most-critical first. Numbering a listed section top-down
    // keeps the author's order after that sort.
    let count = guardrail_texts.len() as i64;
    let guardrails = guardrail_texts
        .into_iter()
        .enumerate()
        .map(|(index, text)| Guardrail {
            id: String::new(),
            number: count - index as i64,
            text,
        })
        .collect();

    DescriptionSections {
        notes: notes.join("\n").trim().to_string(),
        acceptance_criteria: acceptance,
        guardrails,
    }
}

/// The title of a markdown heading line (`## Title` or a lone `**Title**`).
fn heading_title(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    if let Some(rest) = trimmed.strip_prefix('#') {
        let title = rest.trim_start_matches('#').trim();
        return (!title.is_empty()).then_some(title);
    }
    trimmed
        .strip_prefix("**")
        .and_then(|rest| rest.strip_suffix("**"))
        .map(str::trim)
        .filter(|title| !title.is_empty() && !title.contains("**"))
}

/// The text of a bullet, checklist, or numbered list item.
fn list_item(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    for marker in [
        "- [ ] ", "- [x] ", "- [X] ", "* [ ] ", "* [x] ", "- ", "* ", "+ ",
    ] {
        if let Some(item) = trimmed.strip_prefix(marker) {
            let item = item.trim();
            return (!item.is_empty()).then_some(item);
        }
    }
    // "1. item" / "12) item"
    let digits = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 {
        let rest = &trimmed[digits..];
        if let Some(item) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            let item = item.trim();
            return (!item.is_empty()).then_some(item);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn issue_json() -> serde_json::Value {
        json!({
            "id": "issue-uuid",
            "identifier": "ENG-42",
            "title": "Harden the retry loop",
            "description": "Some context.\n\n## Acceptance criteria\n- [ ] retries stop after 3 attempts\n- backoff is exponential\n\n## Guardrails\n1. Do not touch the public API\n2. Keep the feature flag\n\n## Background\nMore prose.",
            "priority": 1,
            "createdAt": "2026-08-01T10:00:00.000Z",
            "updatedAt": "2026-08-02T10:00:00.000Z",
            "archivedAt": null,
            "state": { "name": "Todo", "type": "unstarted" },
            "team": { "id": "team-1" },
            "project": { "id": "proj-epic-1" },
            "assignee": { "displayName": "Ada" },
            "comments": { "nodes": [
                { "id": "c1", "body": "First pass done", "createdAt": "2026-08-01T12:00:00.000Z",
                  "user": { "displayName": "Ada" } }
            ] },
            "inverseRelations": { "nodes": [
                { "type": "blocks", "issue": { "id": "blocker-1" } },
                { "type": "related", "issue": { "id": "unrelated-1" } }
            ] }
        })
    }

    #[test]
    fn an_issue_becomes_a_task_with_its_sections_lifted() {
        let raw: RawIssue = serde_json::from_value(issue_json()).unwrap();
        let task = task_from_issue(raw, "team-1");

        assert_eq!(task.id, "issue-uuid");
        assert_eq!(task.title, "ENG-42 — Harden the retry loop");
        assert_eq!(task.status, "todo");
        assert_eq!(task.project_id, "team-1");
        assert_eq!(task.epic_id.as_deref(), Some("proj-epic-1"));
        assert_eq!(task.priority, Some(Priority::P0));

        // Only the blocking relation becomes a dependency.
        assert_eq!(task.depends_on, vec!["blocker-1"]);

        assert_eq!(
            task.acceptance_criteria,
            vec!["retries stop after 3 attempts", "backoff is exponential"]
        );
        assert_eq!(task.guardrails.len(), 2);
        // First-listed guardrail carries the highest number, so the prompt's
        // most-critical-first sort preserves the author's order.
        assert_eq!(task.guardrails[0].text, "Do not touch the public API");
        assert!(task.guardrails[0].number > task.guardrails[1].number);

        // The lifted sections are gone from the notes; other headings stay.
        let notes = task.notes.unwrap();
        assert!(notes.contains("Some context."));
        assert!(notes.contains("## Background"));
        assert!(!notes.contains("Acceptance criteria"));
        assert!(!notes.contains("feature flag"));

        assert_eq!(task.comments.len(), 1);
        assert_eq!(task.comments[0].author.as_deref(), Some("Ada"));
    }

    #[test]
    fn state_types_map_onto_board_columns() {
        for (state_type, expected) in [
            ("triage", "planning"),
            ("backlog", "planning"),
            ("unstarted", "todo"),
            ("started", "in_progress"),
            ("completed", "done"),
            ("canceled", "canceled"),
        ] {
            assert_eq!(status_from_state_type(state_type), expected);
        }
    }

    #[test]
    fn a_cancelled_dependency_is_not_treated_as_done() {
        // "canceled" deliberately maps to a status the selector does not count
        // as finished, so dependents stay blocked.
        assert_ne!(status_from_state_type("canceled"), "done");
    }

    #[test]
    fn moving_picks_the_leftmost_state_of_the_preferred_type() {
        let states = vec![
            RawWorkflowState {
                id: "later".into(),
                state_type: "started".into(),
                position: 5.0,
            },
            RawWorkflowState {
                id: "first".into(),
                state_type: "started".into(),
                position: 2.0,
            },
            RawWorkflowState {
                id: "todo".into(),
                state_type: "unstarted".into(),
                position: 1.0,
            },
        ];
        assert_eq!(
            pick_state(&states, TaskStatus::InProgress).unwrap().id,
            "first"
        );
        assert_eq!(pick_state(&states, TaskStatus::Todo).unwrap().id, "todo");
        assert!(pick_state(&states, TaskStatus::Done).is_none());
    }

    #[test]
    fn a_team_without_a_backlog_falls_back_for_planning_moves() {
        let states = vec![RawWorkflowState {
            id: "triage".into(),
            state_type: "triage".into(),
            position: 0.0,
        }];
        assert_eq!(
            pick_state(&states, TaskStatus::Planning).unwrap().id,
            "triage"
        );
    }

    #[test]
    fn priorities_collapse_onto_the_three_flux_levels() {
        assert_eq!(priority_from_linear(Some(1)), Some(Priority::P0));
        assert_eq!(priority_from_linear(Some(2)), Some(Priority::P1));
        assert_eq!(priority_from_linear(Some(3)), Some(Priority::P1));
        assert_eq!(priority_from_linear(Some(4)), Some(Priority::P2));
        assert_eq!(priority_from_linear(Some(0)), None);
        assert_eq!(priority_from_linear(None), None);
    }

    #[test]
    fn a_description_with_no_sections_is_all_notes() {
        let sections = parse_description("Just prose.\n\nTwo paragraphs.");
        assert_eq!(sections.notes, "Just prose.\n\nTwo paragraphs.");
        assert!(sections.acceptance_criteria.is_empty());
        assert!(sections.guardrails.is_empty());
    }

    #[test]
    fn a_bold_line_works_as_a_section_heading() {
        let sections = parse_description("**Guardrails**\n- never force-push");
        assert_eq!(sections.guardrails.len(), 1);
        assert_eq!(sections.guardrails[0].text, "never force-push");
        assert!(sections.notes.is_empty());
    }

    #[test]
    fn a_completed_linear_project_reads_as_a_done_epic() {
        let raw = RawEpic {
            id: "epic-1".into(),
            name: "Migration".into(),
            description: None,
            state: Some("completed".into()),
            teams: None,
        };
        let epic = epic_from_project(raw, "team-1", true);
        assert_eq!(epic.status, "done");
        assert!(epic.auto);
        assert_eq!(epic.project_id, "team-1");
    }

    #[test]
    fn a_team_becomes_a_linear_tagged_project() {
        let raw = RawTeam {
            id: "team-1".into(),
            name: "Engineering".into(),
            key: Some("ENG".into()),
            description: None,
        };
        let project = project_from_team(raw);
        assert_eq!(project.source, SourceKind::Linear);
        assert_eq!(project.name, "Engineering (ENG)");
    }
}
