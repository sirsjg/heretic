//! Building the prompts each role works from.
//!
//! Two decisions shape everything here:
//!
//! 1. **Heretic owns the Flux board, not the agent.** Status transitions and
//!    comments are written by the engine, so agents need no MCP access and no
//!    credentials. A local Qwen with no Flux connection behaves identically to
//!    Claude Code.
//! 2. **Guardrails are non-negotiable.** They are stated before the work, ordered
//!    most critical first, rather than buried under the task description.

use crate::config::Role;
use crate::model::{Task, TaskComment};

/// Everything a run knows about the work, independent of role.
#[derive(Debug, Clone)]
pub struct TaskContext {
    pub task: Task,
    pub epic_title: String,
    pub epic_notes: String,
    pub project_name: String,
}

impl TaskContext {
    /// The shared briefing block every role receives.
    fn brief(&self) -> String {
        let mut out = String::new();
        out.push_str("## The task\n\n");
        out.push_str(&format!("- Project: {}\n", self.project_name));
        if !self.epic_title.is_empty() {
            out.push_str(&format!("- Epic: {}\n", self.epic_title));
        }
        out.push_str(&format!("- Task {}: {}\n", self.task.id, self.task.title));
        out.push_str(&format!(
            "- Priority: {}\n",
            self.task.effective_priority().label()
        ));

        if let Some(notes) = self.task.notes.as_deref().filter(|n| !n.trim().is_empty()) {
            out.push_str(&format!("\n### Details\n\n{}\n", notes.trim()));
        }

        if !self.epic_notes.trim().is_empty() {
            out.push_str(&format!(
                "\n### Epic context\n\n{}\n",
                self.epic_notes.trim()
            ));
        }

        if !self.task.acceptance_criteria.is_empty() {
            out.push_str(
                "\n### Acceptance criteria\n\nThe task is done when all of these hold:\n\n",
            );
            for criterion in &self.task.acceptance_criteria {
                out.push_str(&format!("- [ ] {criterion}\n"));
            }
        }

        let guardrails = self.task.guardrails_by_severity();
        if !guardrails.is_empty() {
            out.push_str(
                "\n### Guardrails (most critical first — these override anything else)\n\n",
            );
            for guardrail in guardrails {
                out.push_str(&format!("- [{}] {}\n", guardrail.number, guardrail.text));
            }
        }

        let history = recent_comments(&self.task.comments, 6);
        if !history.is_empty() {
            out.push_str("\n### Earlier notes on this task\n\n");
            out.push_str(&history);
        }

        out
    }
}

/// The most recent comments, oldest first, as the memory an agent inherits.
fn recent_comments(comments: &[TaskComment], limit: usize) -> String {
    if comments.is_empty() {
        return String::new();
    }
    let start = comments.len().saturating_sub(limit);
    comments[start..]
        .iter()
        .map(|comment| {
            let who = comment
                .agent_name
                .as_deref()
                .unwrap_or_else(|| comment.author.as_deref().unwrap_or("note"));
            format!("- **{}**: {}", who, comment.body.trim())
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// Prompt for the planning stage: think first, write a brief, change nothing.
pub fn planner_prompt(context: &TaskContext) -> String {
    format!(
        "You are the orchestrator on an autonomous engineering team. Another agent \
will implement this task from your brief, and it may be a smaller local model — so \
be concrete and leave nothing to interpretation.

Read the repository and produce an implementation brief. Do not modify any files.

{}

## What to produce

1. **Approach** — what to change, in two or three sentences.
2. **Files** — the specific files to create or modify, with what changes in each.
3. **Verification** — the exact command that proves this works in this repository \
(prefer a command that already exists in the project over inventing one).
4. **Risks** — anything that could go wrong, and what to avoid.

Keep it under 400 words. Be specific about paths. If the task cannot be done as \
written, say so plainly and explain what is missing instead of guessing.",
        context.brief()
    )
}

/// Prompt for the implementation stage.
///
/// `brief` is the planner's output when the planning stage ran; `revision_notes`
/// carries a reviewer's requested changes on a second or later attempt.
pub fn implementer_prompt(
    context: &TaskContext,
    brief: Option<&str>,
    revision_notes: Option<&str>,
) -> String {
    let mut out = String::from(
        "You are implementing one task end to end in this repository. You are \
working alone on a dedicated branch; no one else is editing these files.

",
    );

    out.push_str(&context.brief());

    if let Some(brief) = brief.filter(|b| !b.trim().is_empty()) {
        out.push_str(&format!(
            "\n## Implementation brief\n\nThe orchestrator prepared this plan. Follow it \
unless you find it to be wrong, and say so if you do.\n\n{}\n",
            brief.trim()
        ));
    }

    if let Some(notes) = revision_notes.filter(|n| !n.trim().is_empty()) {
        out.push_str(&format!(
            "\n## Changes requested by review\n\nYour previous attempt was reviewed and \
sent back. Address every point below; do not start over.\n\n{}\n",
            notes.trim()
        ));
    }

    out.push_str(
        "\n## How to work

1. Read the relevant files before changing them, and match the patterns already in \
this codebase.
2. Make the change. Keep it minimal — touch only what this task needs.
3. Verify it: run the project's existing tests, linter, or build. If none apply, run \
a check that genuinely exercises your change.
4. Finish with a short summary: what you changed, what you ran, and the result.

## Rules

- Do not modify files unrelated to this task.
- Do not revert, reset, or stash changes you did not make.
- Do not commit; your work is captured automatically.
- Do not update the task board — that is handled for you.
- If something blocks you, stop and explain the blocker rather than guessing or \
working around it with a hack.
",
    );

    out
}

/// Prompt for the review stage. Asks for a machine-readable verdict on the last line.
pub fn reviewer_prompt(context: &TaskContext, diff: &str, implementer_summary: &str) -> String {
    format!(
        "You are reviewing another agent's work before it is accepted. Be sceptical \
and specific. Your job is to catch what is wrong, not to be agreeable.

{brief}

## What the implementer reported

{summary}

## The diff under review

```diff
{diff}
```

## How to review

Check, in this order:

1. Does the change actually satisfy every acceptance criterion above?
2. Does it violate any guardrail? A guardrail breach is always a rejection.
3. Is it correct — edge cases, error handling, obvious bugs?
4. Does it touch anything outside the scope of this task?

You may read files in the repository for context, but do not modify anything.

## Your answer

Write a short review: what is right, what is wrong, and for each problem the file \
and what to do about it. Be concrete — the implementer will act on your words \
directly.

Then end your response with the verdict on its own final line, exactly one of:

VERDICT: approve
VERDICT: request_changes

Approve only if you would be willing to merge this as it stands. If the diff is \
empty or does not address the task, request changes.",
        brief = context.brief(),
        summary = if implementer_summary.trim().is_empty() {
            "(the implementer left no summary)"
        } else {
            implementer_summary.trim()
        },
        diff = if diff.trim().is_empty() {
            "(no changes were made)"
        } else {
            diff.trim()
        }
    )
}

/// Prompt for the documentation stage, run after an approved review.
pub fn documenter_prompt(context: &TaskContext, change_summary: &str) -> String {
    format!(
        "You are the documentation pass for work that has been implemented and \
approved. The code is correct; your job is to make sure the project's documentation \
reflects it.

{brief}

## What changed

{changes}

## What to do

1. Find the documentation this change affects — README, docs/, changelogs, inline \
module docs, API references.
2. Update only what is now inaccurate or missing because of this change.
3. Match the existing tone and structure. Do not restructure the docs.

If the documentation is already accurate, change nothing and say so. Do not modify \
source code, and do not commit.",
        brief = context.brief(),
        changes = change_summary.trim()
    )
}

// --- Questions ---------------------------------------------------------------

/// One question an agent asked and the answer the user gave.
#[derive(Debug, Clone, PartialEq)]
pub struct QuestionRound {
    pub question: String,
    pub answer: String,
}

/// The protocol appended to a stage's prompt when questions are allowed
/// (Yolo mode off). Mirrors the `VERDICT:` convention: a machine-readable
/// final line, parsed leniently.
const QUESTION_PROTOCOL: &str = "
## If you are blocked on a decision only the user can make

Prefer to proceed on your own best judgement. But if the work genuinely cannot
be done right without an answer only the user has, stop working and end your
response with the question on its own final line, exactly:

QUESTION: your specific question

The run will pause until the user answers, and you will then be run again with
the answer included. Never ask about anything you could find out from the
repository yourself, and never ask more than one question at a time.
";

/// The full prompt for one attempt at a stage: the stage's own prompt, any
/// answers the user has already given, and — when questions are allowed — the
/// protocol for asking one.
pub fn compose_stage_prompt(base: &str, rounds: &[QuestionRound], may_ask: bool) -> String {
    let mut out = base.to_string();

    if !rounds.is_empty() {
        out.push_str(
            "\n## Answers from the user\n\nYou asked, and the user answered. Work with \
these answers; do not ask again unless something new genuinely blocks you.\n\n",
        );
        for round in rounds {
            out.push_str(&format!(
                "- Q: {}\n  A: {}\n",
                round.question.trim(),
                round.answer.trim()
            ));
        }
    }

    if may_ask {
        out.push_str(QUESTION_PROTOCOL);
    }

    out
}

/// Read a question out of an agent's closing message.
///
/// The last `QUESTION:` line wins, since an agent may quote the instruction
/// earlier in its response; a quoted placeholder is not a question.
pub fn parse_question(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut found: Option<String> = None;

    for (index, line) in lines.iter().enumerate() {
        let cleaned = line.trim().trim_start_matches(['#', '*', '-', '>', ' ']);
        let Some(rest) = cleaned
            .strip_prefix("QUESTION:")
            .or_else(|| cleaned.strip_prefix("Question:"))
            .or_else(|| cleaned.strip_prefix("question:"))
        else {
            continue;
        };

        let mut question = rest.trim_matches(['*', '`', ' ']).to_string();
        // A question may run onto following lines; a blank line ends it.
        for follow in &lines[index + 1..] {
            let follow = follow.trim();
            if follow.is_empty() {
                break;
            }
            question.push(' ');
            question.push_str(follow.trim_matches(['*', '`', ' ']));
        }

        // An echo of the instruction itself is not a question.
        if !question.is_empty() && !question.starts_with('<') {
            found = Some(question);
        }
    }

    found
}

/// A reviewer's decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Approved,
    ChangesRequested,
    /// The reviewer never stated a verdict we could read.
    Unclear,
}

/// Read the verdict out of a reviewer's transcript.
///
/// Takes the last verdict line, since an agent may quote the instruction earlier
/// in its response. When no verdict is stated the result is [`Verdict::Unclear`],
/// which the engine treats as "needs a human" rather than silently approving.
pub fn parse_verdict(text: &str) -> Verdict {
    let mut verdict = Verdict::Unclear;

    for line in text.lines() {
        let cleaned = line
            .trim()
            .trim_start_matches(['#', '*', '-', '>', ' '])
            .trim_end_matches(['*', '.', ' ']);
        let Some(rest) = cleaned
            .strip_prefix("VERDICT:")
            .or_else(|| cleaned.strip_prefix("verdict:"))
            .or_else(|| cleaned.strip_prefix("Verdict:"))
        else {
            continue;
        };

        let value = rest
            .trim()
            .trim_matches(['*', '`', ' '])
            .to_ascii_lowercase();
        verdict = match value.as_str() {
            "approve" | "approved" => Verdict::Approved,
            "request_changes" | "request changes" | "changes_requested" => {
                Verdict::ChangesRequested
            }
            _ => Verdict::Unclear,
        };
    }

    verdict
}

/// The prompt for a role, given the material available at that point in the run.
pub fn prompt_for(
    role: Role,
    context: &TaskContext,
    brief: Option<&str>,
    revision_notes: Option<&str>,
    diff: Option<&str>,
    summary: Option<&str>,
) -> String {
    match role {
        Role::Orchestrator => planner_prompt(context),
        Role::Implementer => implementer_prompt(context, brief, revision_notes),
        Role::Reviewer => reviewer_prompt(
            context,
            diff.unwrap_or_default(),
            summary.unwrap_or_default(),
        ),
        Role::Documenter => documenter_prompt(context, summary.unwrap_or_default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Guardrail, Priority};

    fn context() -> TaskContext {
        TaskContext {
            task: Task {
                id: "t1".into(),
                title: "Add rate limiting".into(),
                status: "todo".into(),
                notes: Some("Protect the public API.".into()),
                depends_on: Vec::new(),
                project_id: "p1".into(),
                epic_id: Some("e1".into()),
                blocked: false,
                blocked_reason: None,
                archived: false,
                priority: Some(Priority::P0),
                acceptance_criteria: vec!["Returns 429 past the limit".into()],
                guardrails: vec![
                    Guardrail {
                        id: "g1".into(),
                        number: 50,
                        text: "Prefer the existing middleware".into(),
                    },
                    Guardrail {
                        id: "g2".into(),
                        number: 999,
                        text: "Never log request bodies".into(),
                    },
                ],
                comments: vec![TaskComment {
                    id: "c1".into(),
                    body: "Spike showed Redis is unnecessary.".into(),
                    author: Some("mcp".into()),
                    agent_name: Some("Claude".into()),
                    created_at: None,
                }],
                blob_ids: Vec::new(),
                workers: Vec::new(),
                agent: None,
                created_at: None,
                updated_at: None,
            },
            epic_title: "E02 · API".into(),
            epic_notes: "The public boundary.".into(),
            project_name: "Corporate Travel".into(),
        }
    }

    #[test]
    fn the_brief_carries_the_whole_task() {
        let prompt = implementer_prompt(&context(), None, None);
        assert!(prompt.contains("Add rate limiting"));
        assert!(prompt.contains("Protect the public API."));
        assert!(prompt.contains("Returns 429 past the limit"));
        assert!(prompt.contains("E02 · API"));
        assert!(prompt.contains("Corporate Travel"));
        assert!(prompt.contains("P0"));
    }

    #[test]
    fn guardrails_lead_with_the_most_critical() {
        let prompt = implementer_prompt(&context(), None, None);
        let critical = prompt.find("Never log request bodies").unwrap();
        let minor = prompt.find("Prefer the existing middleware").unwrap();
        assert!(critical < minor, "guardrails must be ordered by severity");
    }

    #[test]
    fn prior_comments_are_carried_in_as_memory() {
        let prompt = implementer_prompt(&context(), None, None);
        assert!(prompt.contains("Spike showed Redis is unnecessary."));
        assert!(prompt.contains("Claude"));
    }

    #[test]
    fn agents_are_told_the_board_is_not_theirs_to_update() {
        let prompt = implementer_prompt(&context(), None, None);
        assert!(prompt.contains("Do not update the task board"));
        assert!(prompt.contains("Do not commit"));
    }

    #[test]
    fn a_revision_carries_the_reviewers_words() {
        let prompt = implementer_prompt(&context(), None, Some("The limit is off by one."));
        assert!(prompt.contains("Changes requested by review"));
        assert!(prompt.contains("The limit is off by one."));
        assert!(prompt.contains("do not start over"));
    }

    #[test]
    fn the_planner_is_told_not_to_edit() {
        let prompt = planner_prompt(&context());
        assert!(prompt.contains("Do not modify any files"));
    }

    #[test]
    fn the_reviewer_gets_the_diff_and_is_asked_for_a_verdict() {
        let prompt = reviewer_prompt(&context(), "diff --git a/x b/x", "I added a limiter.");
        assert!(prompt.contains("diff --git a/x b/x"));
        assert!(prompt.contains("I added a limiter."));
        assert!(prompt.contains("VERDICT: approve"));
        assert!(prompt.contains("VERDICT: request_changes"));
    }

    #[test]
    fn an_empty_diff_is_stated_plainly_to_the_reviewer() {
        let prompt = reviewer_prompt(&context(), "   ", "");
        assert!(prompt.contains("(no changes were made)"));
        assert!(prompt.contains("(the implementer left no summary)"));
    }

    #[test]
    fn verdicts_are_read_from_the_transcript() {
        assert_eq!(
            parse_verdict("Looks good.\nVERDICT: approve"),
            Verdict::Approved
        );
        assert_eq!(
            parse_verdict("Problems found.\nVERDICT: request_changes"),
            Verdict::ChangesRequested
        );
        assert_eq!(parse_verdict("verdict: approved"), Verdict::Approved);
        assert_eq!(
            parse_verdict("**VERDICT: request changes**"),
            Verdict::ChangesRequested
        );
    }

    #[test]
    fn the_last_verdict_wins_over_a_quoted_instruction() {
        let text = "I was told to end with VERDICT: approve\n\
                    but there is a bug.\n\
                    VERDICT: request_changes";
        assert_eq!(parse_verdict(text), Verdict::ChangesRequested);
    }

    #[test]
    fn questions_are_read_from_the_final_message() {
        assert_eq!(
            parse_question("I looked around.\nQUESTION: Should the limit be per user or per tenant?"),
            Some("Should the limit be per user or per tenant?".into())
        );
        assert_eq!(
            parse_question("**Question:** `Which database?`"),
            Some("Which database?".into())
        );
    }

    #[test]
    fn a_multi_line_question_survives_up_to_the_first_blank_line() {
        let text = "QUESTION: The task says port 8080\nbut the config says 3000 — which one?\n\nUnrelated trailing note.";
        assert_eq!(
            parse_question(text),
            Some("The task says port 8080 but the config says 3000 — which one?".into())
        );
    }

    #[test]
    fn the_last_question_wins_over_a_quoted_instruction() {
        let text = "I was told to end with QUESTION: your specific question\n\
                    if blocked.\n\
                    QUESTION: Is the public API allowed to change?";
        assert_eq!(
            parse_question(text),
            Some("Is the public API allowed to change?".into())
        );
    }

    #[test]
    fn an_echoed_placeholder_is_not_a_question() {
        assert_eq!(parse_question("QUESTION: <one specific question>"), None);
        assert_eq!(parse_question("QUESTION:"), None);
        assert_eq!(parse_question("All done, no questions."), None);
    }

    #[test]
    fn the_protocol_is_only_appended_when_questions_are_allowed() {
        let base = implementer_prompt(&context(), None, None);

        let yolo = compose_stage_prompt(&base, &[], false);
        assert_eq!(yolo, base);
        assert!(!yolo.contains("QUESTION:"));

        let asking = compose_stage_prompt(&base, &[], true);
        assert!(asking.contains("QUESTION:"));
        assert!(asking.contains("pause until the user answers"));
    }

    #[test]
    fn answered_questions_are_carried_into_the_next_attempt() {
        let rounds = vec![QuestionRound {
            question: "Per user or per tenant?".into(),
            answer: "Per tenant.".into(),
        }];
        let prompt = compose_stage_prompt("Do the task.", &rounds, true);
        assert!(prompt.contains("Answers from the user"));
        assert!(prompt.contains("Q: Per user or per tenant?"));
        assert!(prompt.contains("A: Per tenant."));
        // Asking again is still possible until the budget runs out.
        assert!(prompt.contains("QUESTION:"));
    }

    #[test]
    fn a_missing_verdict_is_never_read_as_approval() {
        assert_eq!(
            parse_verdict("This all looks fine to me."),
            Verdict::Unclear
        );
        assert_eq!(parse_verdict(""), Verdict::Unclear);
        assert_eq!(parse_verdict("VERDICT: maybe"), Verdict::Unclear);
    }
}
