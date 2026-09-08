//! Push notifications for the moments a run needs a person.
//!
//! A run that is working needs nobody; a run that has stopped to ask a
//! question, failed, or left work on a branch does. Those transitions are the
//! only ones reported here, once each, so a phone buzzes for a decision and
//! never for progress.
//!
//! Delivery is a plain HTTP post to ntfy or Pushover, whichever is configured.
//! Neither service is contacted unless it is, and a delivery failure is logged
//! rather than surfaced: the run itself is unaffected, and the person will
//! see the state next time they look.

use crate::config::{NotifyConfig, NtfyConfig, PushoverConfig};
use crate::orchestrator::{Engine, EngineEvent, Landing, RunRecord, RunResult, RunStatus};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;

/// What a notification says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub title: String,
    pub body: String,
    /// Whether this needs someone now, as opposed to when they next look.
    pub urgent: bool,
    /// Where the run can be opened, when a remote address is known.
    pub link: Option<String>,
}

/// Decide whether moving from `before` to `after` is worth a notification.
///
/// `before` is `None` for a run seen for the first time; runs already finished
/// when Heretic started are not re-announced.
pub fn transition(
    before: Option<RunStatus>,
    after: &RunRecord,
    on_success: bool,
) -> Option<Notification> {
    if before == Some(after.status) {
        return None;
    }
    // A run first seen already at rest is history, not news.
    if before.is_none() && !after.is_active() {
        return None;
    }

    let title = if after.project_name.is_empty() {
        after.task_title.clone()
    } else {
        format!("{} · {}", after.project_name, after.task_title)
    };

    match after.status {
        RunStatus::Waiting => {
            let question = after
                .question
                .as_ref()
                .map(|q| q.question.trim().to_string())
                .filter(|q| !q.is_empty())
                .unwrap_or_else(|| "An agent has a question.".into());
            Some(Notification {
                title,
                body: format!("Waiting for you: {question}"),
                urgent: true,
                link: None,
            })
        }
        RunStatus::NeedsAttention => {
            let reason = match &after.result {
                Some(RunResult::NeedsAttention { reason }) => reason.clone(),
                Some(result) => result.describe(),
                None => "Needs a look.".into(),
            };
            Some(Notification {
                title,
                body: format!("Needs attention: {reason}"),
                urgent: true,
                link: None,
            })
        }
        RunStatus::Failed => {
            let reason = match &after.result {
                Some(result) => result.describe(),
                None => "Failed.".into(),
            };
            Some(Notification {
                title,
                body: reason,
                urgent: false,
                link: None,
            })
        }
        RunStatus::Succeeded => {
            // Work left on a branch is a decision waiting to be made, which is
            // worth a buzz even when plain successes are muted.
            let body = match (after.landing, after.branch.as_deref()) {
                (Landing::OnBranch, Some(branch)) => {
                    format!("Approved and committed to {branch} — merge or discard it.")
                }
                (Landing::OnBranch, None) => "Approved — the work is waiting to be merged.".into(),
                (Landing::Merged, _) => "Approved and merged.".into(),
                _ => "Completed.".into(),
            };
            if after.landing != Landing::OnBranch && !on_success {
                return None;
            }
            Some(Notification {
                title,
                body,
                urgent: false,
                link: None,
            })
        }
        RunStatus::Queued | RunStatus::Running | RunStatus::Cancelled => None,
    }
}

/// Follow the engine for as long as it runs, posting each transition that
/// deserves it.
pub async fn watch(engine: Arc<Engine>) {
    let mut events = engine.subscribe();
    let mut last: HashMap<String, RunStatus> = HashMap::new();

    // Runs that exist already are the starting point, not news.
    for run in engine.runs().await {
        last.insert(run.id, run.status);
    }

    loop {
        let run = match events.recv().await {
            Ok(EngineEvent::RunUpdated { run }) => run,
            Ok(_) => continue,
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => break,
        };

        let before = last.insert(run.id.clone(), run.status);
        let settings = engine.settings().await;
        if !settings.notifications.enabled() {
            continue;
        }

        let Some(mut notification) = transition(before, &run, settings.notifications.on_success)
        else {
            continue;
        };
        notification.link = settings
            .remote
            .public_url
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty())
            .map(|url| format!("{}/#run={}", url.trim_end_matches('/'), run.id));

        let config = settings.notifications.clone();
        tokio::spawn(async move {
            deliver(&config, &notification).await;
        });
    }
}

/// Post one notification to every configured service.
pub async fn deliver(config: &NotifyConfig, notification: &Notification) {
    if let Some(ntfy) = config.ntfy.as_ref().filter(|n| n.enabled()) {
        if let Err(error) = post_ntfy(ntfy, notification).await {
            tracing::warn!(%error, "could not post to ntfy");
        }
    }
    if let Some(pushover) = config.pushover.as_ref().filter(|p| p.enabled()) {
        if let Err(error) = post_pushover(pushover, notification).await {
            tracing::warn!(%error, "could not post to Pushover");
        }
    }
}

/// Post a test message, returning what went wrong so Settings can show it.
pub async fn deliver_test(config: &NotifyConfig) -> Result<(), String> {
    let notification = Notification {
        title: "Heretic".into(),
        body: "Notifications are working. You will hear from me when a run needs you.".into(),
        urgent: false,
        link: None,
    };
    let mut failures = Vec::new();
    if let Some(ntfy) = config.ntfy.as_ref().filter(|n| n.enabled()) {
        if let Err(error) = post_ntfy(ntfy, &notification).await {
            failures.push(format!("ntfy: {error}"));
        }
    }
    if let Some(pushover) = config.pushover.as_ref().filter(|p| p.enabled()) {
        if let Err(error) = post_pushover(pushover, &notification).await {
            failures.push(format!("Pushover: {error}"));
        }
    }
    if !config.enabled() {
        return Err("No notification service is configured.".into());
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join(" "))
    }
}

fn http() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|error| error.to_string())
}

async fn post_ntfy(config: &NtfyConfig, notification: &Notification) -> Result<(), String> {
    let url = format!(
        "{}/{}",
        config.server.trim().trim_end_matches('/'),
        config.topic.trim()
    );
    let mut request = http()?
        .post(&url)
        .header("Title", header_safe(&notification.title))
        .header(
            "Priority",
            if notification.urgent {
                "high"
            } else {
                "default"
            },
        )
        .header("Tags", if notification.urgent { "bell" } else { "robot" })
        .body(notification.body.clone());
    if let Some(link) = &notification.link {
        request = request.header("Click", link.clone());
    }
    if let Some(token) = config
        .token
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        request = request.bearer_auth(token);
    }
    let response = request.send().await.map_err(|error| error.to_string())?;
    let status = response.status();
    if status.is_success() {
        Ok(())
    } else {
        let text = response.text().await.unwrap_or_default();
        Err(format!("{url} answered {status}: {}", text.trim()))
    }
}

async fn post_pushover(config: &PushoverConfig, notification: &Notification) -> Result<(), String> {
    let mut form: Vec<(&str, String)> = vec![
        ("token", config.app_token.trim().to_string()),
        ("user", config.user_key.trim().to_string()),
        ("title", notification.title.clone()),
        ("message", notification.body.clone()),
        (
            "priority",
            if notification.urgent { "1" } else { "0" }.to_string(),
        ),
    ];
    if let Some(link) = &notification.link {
        form.push(("url", link.clone()));
        form.push(("url_title", "Open in Heretic".into()));
    }
    let response = http()?
        .post("https://api.pushover.net/1/messages.json")
        .form(&form)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    let status = response.status();
    if status.is_success() {
        Ok(())
    } else {
        let text = response.text().await.unwrap_or_default();
        Err(format!("Pushover answered {status}: {}", text.trim()))
    }
}

/// HTTP headers cannot carry a newline or anything outside Latin-1; ntfy takes
/// the title as one, so flatten what does not fit.
fn header_safe(text: &str) -> String {
    text.chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .filter(|c| (*c as u32) < 256)
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator::{PendingQuestion, RunStage};

    fn run(status: RunStatus) -> RunRecord {
        RunRecord {
            id: "r1".into(),
            project_id: "p".into(),
            project_name: "Flux".into(),
            task_id: "t".into(),
            task_title: "Add search".into(),
            epic_title: String::new(),
            status,
            stage: RunStage::Implementing,
            agent: None,
            started_at: String::new(),
            finished_at: None,
            revisions: 0,
            branch: Some("heretic/add-search".into()),
            base_branch: None,
            worktree_path: None,
            question: None,
            landing: Landing::Nothing,
            changes: Default::default(),
            result: None,
            stats: Vec::new(),
            feed: Vec::new(),
        }
    }

    #[test]
    fn progress_is_not_news() {
        assert!(transition(None, &run(RunStatus::Queued), true).is_none());
        assert!(transition(Some(RunStatus::Queued), &run(RunStatus::Running), true).is_none());
        assert!(transition(Some(RunStatus::Running), &run(RunStatus::Running), true).is_none());
        assert!(transition(Some(RunStatus::Running), &run(RunStatus::Cancelled), true).is_none());
    }

    #[test]
    fn a_question_is_urgent_and_quoted() {
        let mut waiting = run(RunStatus::Waiting);
        waiting.question = Some(PendingQuestion {
            stage: RunStage::Implementing,
            role: None,
            question: "Postgres or SQLite?".into(),
        });
        let n = transition(Some(RunStatus::Running), &waiting, false).unwrap();
        assert!(n.urgent);
        assert_eq!(n.title, "Flux · Add search");
        assert_eq!(n.body, "Waiting for you: Postgres or SQLite?");
    }

    #[test]
    fn a_run_seen_first_at_rest_is_history() {
        let mut done = run(RunStatus::Succeeded);
        done.landing = Landing::OnBranch;
        assert!(transition(None, &done, true).is_none());
    }

    #[test]
    fn work_left_on_a_branch_is_announced_even_when_successes_are_muted() {
        let mut done = run(RunStatus::Succeeded);
        done.landing = Landing::OnBranch;
        let n = transition(Some(RunStatus::Running), &done, false).unwrap();
        assert!(n.body.contains("heretic/add-search"));

        let mut merged = run(RunStatus::Succeeded);
        merged.landing = Landing::Merged;
        assert!(transition(Some(RunStatus::Running), &merged, false).is_none());
        assert!(transition(Some(RunStatus::Running), &merged, true).is_some());
    }

    #[test]
    fn a_failure_says_where() {
        let mut failed = run(RunStatus::Failed);
        failed.result = Some(RunResult::Failed {
            stage: RunStage::Reviewing,
            reason: "the reviewer timed out".into(),
        });
        let n = transition(Some(RunStatus::Running), &failed, false).unwrap();
        assert_eq!(n.body, "Failed during reviewing: the reviewer timed out");
        assert!(!n.urgent);
    }

    #[test]
    fn titles_fit_in_a_header() {
        assert_eq!(header_safe("Flux · line\nbreak  "), "Flux · line break");
        assert_eq!(header_safe("emoji 🔥 gone"), "emoji  gone");
    }
}
