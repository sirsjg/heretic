//! GraphQL client for the Linear API, presented as a [`TaskSource`].

use super::map::{self, Nodes, Page, RawEpic, RawIssue, RawTeam, RawWorkflowState};
use crate::model::{Epic, Project, SourceKind, Task, TaskStatus};
use crate::source::{Result, SourceError, SourceErrorKind, TaskSource};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashSet;

/// Connection settings for a Linear workspace.
///
/// An API key is scoped to one workspace, so one configuration covers every
/// team in it. `auto_epics` lives here rather than on Linear because Linear
/// has no equivalent of Flux's `auto` flag — the switch that authorises
/// unattended work stays Heretic's own, stored with the connection.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, Deserialize)]
#[serde(default)]
pub struct LinearConfig {
    /// A personal API key (`lin_api_…`), created under Linear's
    /// Settings → Security & access → API keys.
    pub api_key: Option<String>,
    /// The GraphQL endpoint. Only ever changed to point tests at a stub.
    pub base_url: String,
    /// Epic ids (Linear project ids) whose tasks may run unattended.
    pub auto_epics: Vec<String>,
}

impl Default for LinearConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            base_url: "https://api.linear.app/graphql".to_string(),
            auto_epics: Vec::new(),
        }
    }
}

impl LinearConfig {
    /// Whether the user has set this connection up at all.
    pub fn enabled(&self) -> bool {
        self.api_key
            .as_deref()
            .is_some_and(|k| !k.trim().is_empty())
    }
}

/// How many issues to read per page, and how many pages before giving up.
/// 400 issues per team is far past the point where a board stops being a
/// queue; past it Heretic works the newest pages rather than stalling.
const PAGE_SIZE: u32 = 100;
const MAX_PAGES: u32 = 4;

#[derive(Debug, Clone)]
pub struct LinearClient {
    config: LinearConfig,
    auto_epics: HashSet<String>,
    http: reqwest::Client,
}

impl LinearClient {
    pub fn new(config: LinearConfig) -> Result<Self> {
        if !config.enabled() {
            return Err(SourceError::new(
                SourceErrorKind::Unconfigured,
                "Linear is not connected — add an API key in Settings.",
            ));
        }
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| SourceError::new(SourceErrorKind::Transport, e.to_string()))?;
        let auto_epics = config.auto_epics.iter().cloned().collect();
        Ok(Self {
            config,
            auto_epics,
            http,
        })
    }

    /// One GraphQL round trip. `T` is the shape under `data`.
    async fn query<T: serde::de::DeserializeOwned>(
        &self,
        query: &str,
        variables: serde_json::Value,
    ) -> Result<T> {
        let key = self.config.api_key.as_deref().unwrap_or_default().trim();

        let response = self
            .http
            .post(&self.config.base_url)
            // A personal API key is sent bare; only OAuth tokens take `Bearer`.
            .header(reqwest::header::AUTHORIZATION, key)
            .json(&json!({ "query": query, "variables": variables }))
            .send()
            .await
            .map_err(|e| {
                SourceError::new(
                    SourceErrorKind::Transport,
                    format!("Linear request failed: {e}"),
                )
            })?;

        let status = response.status().as_u16();
        let body = response
            .text()
            .await
            .map_err(|e| SourceError::new(SourceErrorKind::Transport, e.to_string()))?;

        if status == 401 || status == 403 {
            return Err(SourceError::new(
                SourceErrorKind::Auth,
                "Linear rejected the API key. Check it in Settings.",
            ));
        }

        let envelope: GraphQlResponse<T> = serde_json::from_str(&body).map_err(|e| {
            SourceError::new(
                SourceErrorKind::Decode,
                format!("Linear returned a response Heretic could not parse: {e}"),
            )
        })?;

        if let Some(errors) = envelope.errors.filter(|e| !e.is_empty()) {
            let message = errors
                .iter()
                .map(|e| e.message.as_str())
                .collect::<Vec<_>>()
                .join("; ");
            let kind = if errors.iter().any(GraphQlError::is_auth) {
                SourceErrorKind::Auth
            } else {
                SourceErrorKind::Api
            };
            return Err(SourceError::new(kind, format!("Linear: {message}")));
        }

        envelope.data.ok_or_else(|| {
            SourceError::new(
                SourceErrorKind::Decode,
                "Linear answered without data or errors.",
            )
        })
    }

    /// Whoami — the cheapest authenticated request, used by the connection test.
    pub async fn viewer_name(&self) -> Result<String> {
        let data: ViewerData = self
            .query("query { viewer { name email } }", json!({}))
            .await?;
        Ok(data.viewer.name.or(data.viewer.email).unwrap_or_default())
    }

    fn not_found(what: &str, id: &str) -> SourceError {
        SourceError::new(
            SourceErrorKind::NotFound,
            format!("Linear has no {what} with id {id}."),
        )
    }

    async fn issue_workflow_states(&self, issue_id: &str) -> Result<Vec<RawWorkflowState>> {
        let data: IssueStatesData = self
            .query(
                "query($id: String!) { issue(id: $id) { id team { id states(first: 50) { nodes { id type position } } } } }",
                json!({ "id": issue_id }),
            )
            .await?;
        let issue = data
            .issue
            .ok_or_else(|| Self::not_found("issue", issue_id))?;
        Ok(issue.team.map(|team| team.states.nodes).unwrap_or_default())
    }

    async fn set_issue_state(&self, issue_id: &str, status: TaskStatus) -> Result<()> {
        let states = self.issue_workflow_states(issue_id).await?;
        let state = map::pick_state(&states, status).ok_or_else(|| {
            SourceError::new(
                SourceErrorKind::Api,
                format!(
                    "This Linear team has no workflow state for \"{}\".",
                    status.as_str()
                ),
            )
        })?;

        let data: IssueUpdateData = self
            .query(
                "mutation($id: String!, $stateId: String!) { issueUpdate(id: $id, input: { stateId: $stateId }) { success } }",
                json!({ "id": issue_id, "stateId": state.id }),
            )
            .await?;
        if !data.issue_update.success {
            return Err(SourceError::new(
                SourceErrorKind::Api,
                "Linear did not accept the status change.",
            ));
        }
        Ok(())
    }

    async fn post_comment(&self, issue_id: &str, body: String) -> Result<()> {
        let data: CommentCreateData = self
            .query(
                "mutation($id: String!, $body: String!) { commentCreate(input: { issueId: $id, body: $body }) { success } }",
                json!({ "id": issue_id, "body": body }),
            )
            .await?;
        if !data.comment_create.success {
            return Err(SourceError::new(
                SourceErrorKind::Api,
                "Linear did not accept the comment.",
            ));
        }
        Ok(())
    }
}

const ISSUE_FIELDS: &str =
    "id identifier title description priority createdAt updatedAt archivedAt \
state { type } team { id } project { id } \
comments(first: 50) { nodes { id body createdAt user { displayName name } botActor { name } } } \
inverseRelations(first: 50) { nodes { type issue { id state { type } } } }";

#[async_trait::async_trait]
impl TaskSource for LinearClient {
    fn kind(&self) -> SourceKind {
        SourceKind::Linear
    }

    async fn list_projects(&self) -> Result<Vec<Project>> {
        let data: TeamsData = self
            .query(
                "query { teams(first: 100) { nodes { id name key description } } }",
                json!({}),
            )
            .await?;
        Ok(data
            .teams
            .nodes
            .into_iter()
            .map(map::project_from_team)
            .collect())
    }

    async fn get_project(&self, project_id: &str) -> Result<Project> {
        let data: TeamData = self
            .query(
                "query($id: String!) { team(id: $id) { id name key description } }",
                json!({ "id": project_id }),
            )
            .await?;
        data.team
            .map(map::project_from_team)
            .ok_or_else(|| Self::not_found("team", project_id))
    }

    async fn list_epics(&self, project_id: &str) -> Result<Vec<Epic>> {
        let data: TeamProjectsData = self
            .query(
                "query($id: String!) { team(id: $id) { id projects(first: 100) { nodes { id name description state } } } }",
                json!({ "id": project_id }),
            )
            .await?;
        let team = data
            .team
            .ok_or_else(|| Self::not_found("team", project_id))?;
        Ok(team
            .projects
            .nodes
            .into_iter()
            .map(|raw| {
                let auto = self.auto_epics.contains(&raw.id);
                map::epic_from_project(raw, project_id, auto)
            })
            .collect())
    }

    async fn get_epic(&self, epic_id: &str) -> Result<Epic> {
        let data: ProjectData = self
            .query(
                "query($id: String!) { project(id: $id) { id name description state teams(first: 1) { nodes { id } } } }",
                json!({ "id": epic_id }),
            )
            .await?;
        let raw = data
            .project
            .ok_or_else(|| Self::not_found("project", epic_id))?;
        let team_id = raw
            .teams
            .as_ref()
            .and_then(|teams| teams.nodes.first())
            .map(|team| team.id.clone())
            .unwrap_or_default();
        let auto = self.auto_epics.contains(&raw.id);
        Ok(map::epic_from_project(raw, &team_id, auto))
    }

    /// Linear has nowhere to keep this flag, so it lives in Heretic's settings;
    /// the command layer updates them and rebuilds this client.
    async fn set_epic_auto(&self, _epic_id: &str, _auto: bool) -> Result<()> {
        Err(SourceError::new(
            SourceErrorKind::Api,
            "Linear has no auto flag of its own — Heretic stores it in settings.",
        ))
    }

    async fn list_tasks(&self, project_id: &str) -> Result<Vec<Task>> {
        let mut tasks = Vec::new();
        let mut after: Option<String> = None;

        for _ in 0..MAX_PAGES {
            let data: TeamIssuesData = self
                .query(
                    &format!(
                        "query($id: String!, $first: Int!, $after: String) {{ team(id: $id) {{ id issues(first: $first, after: $after) {{ pageInfo {{ hasNextPage endCursor }} nodes {{ {ISSUE_FIELDS} }} }} }} }}"
                    ),
                    json!({ "id": project_id, "first": PAGE_SIZE, "after": after }),
                )
                .await?;
            let team = data
                .team
                .ok_or_else(|| Self::not_found("team", project_id))?;

            let page = team.issues;
            tasks.extend(
                page.nodes
                    .into_iter()
                    .map(|issue| map::task_from_issue(issue, project_id)),
            );

            if !page.page_info.has_next_page {
                break;
            }
            after = page.page_info.end_cursor;
            if after.is_none() {
                break;
            }
        }

        Ok(tasks)
    }

    async fn get_task(&self, task_id: &str) -> Result<Task> {
        let data: IssueData = self
            .query(
                &format!("query($id: String!) {{ issue(id: $id) {{ {ISSUE_FIELDS} }} }}"),
                json!({ "id": task_id }),
            )
            .await?;
        let issue = data
            .issue
            .ok_or_else(|| Self::not_found("issue", task_id))?;
        let team_id = issue
            .team
            .as_ref()
            .map(|t| t.id.clone())
            .unwrap_or_default();
        Ok(map::task_from_issue(issue, &team_id))
    }

    async fn move_status(
        &self,
        task_id: &str,
        status: TaskStatus,
        _agent_name: Option<&str>,
    ) -> Result<()> {
        // Linear has no worker badge; attribution rides on comments instead.
        self.set_issue_state(task_id, status).await
    }

    async fn comment(&self, task_id: &str, body: &str, agent_name: Option<&str>) -> Result<()> {
        // Comments land as the API key's user, so name the agent in the body.
        let body = match agent_name {
            Some(name) => format!("**{name}**\n\n{body}"),
            None => body.to_string(),
        };
        self.post_comment(task_id, body).await
    }

    /// Linear has no blocker field, so the *intent* is honoured instead: the
    /// reason is recorded as a comment and the issue is moved back to the
    /// backlog, which takes it out of Todo and therefore out of the auto
    /// loop's reach until a human triages it.
    async fn set_blocked_reason(&self, task_id: &str, reason: Option<&str>) -> Result<()> {
        let Some(reason) = reason else {
            // Clearing is a no-op: starting a run claims the issue into
            // In Progress, which already leaves nothing to clear.
            return Ok(());
        };
        self.post_comment(task_id, format!("⛔ **Blocked** — {reason}"))
            .await?;
        self.set_issue_state(task_id, TaskStatus::Planning).await
    }
}

// --- Response envelopes -------------------------------------------------------

#[derive(Deserialize)]
struct GraphQlResponse<T> {
    data: Option<T>,
    errors: Option<Vec<GraphQlError>>,
}

#[derive(Deserialize)]
struct GraphQlError {
    #[serde(default)]
    message: String,
    #[serde(default)]
    extensions: Option<serde_json::Value>,
}

impl GraphQlError {
    fn is_auth(&self) -> bool {
        let coded = self
            .extensions
            .as_ref()
            .and_then(|e| e.get("code"))
            .and_then(|c| c.as_str())
            .is_some_and(|code| code.contains("AUTHENTICATION") || code.contains("FORBIDDEN"));
        coded || self.message.to_lowercase().contains("authentication")
    }
}

#[derive(Deserialize)]
struct ViewerData {
    viewer: Viewer,
}

#[derive(Deserialize)]
struct Viewer {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    email: Option<String>,
}

#[derive(Deserialize)]
struct TeamsData {
    #[serde(default)]
    teams: Nodes<RawTeam>,
}

#[derive(Deserialize)]
struct TeamData {
    team: Option<RawTeam>,
}

#[derive(Deserialize)]
struct TeamProjectsData {
    team: Option<TeamProjects>,
}

#[derive(Deserialize)]
struct TeamProjects {
    #[serde(default)]
    projects: Nodes<RawEpic>,
}

#[derive(Deserialize)]
struct ProjectData {
    project: Option<RawEpic>,
}

#[derive(Deserialize)]
struct TeamIssuesData {
    team: Option<TeamIssues>,
}

#[derive(Deserialize)]
struct TeamIssues {
    issues: Page<RawIssue>,
}

#[derive(Deserialize)]
struct IssueData {
    issue: Option<RawIssue>,
}

#[derive(Deserialize)]
struct IssueStatesData {
    issue: Option<IssueWithStates>,
}

#[derive(Deserialize)]
struct IssueWithStates {
    team: Option<TeamWithStates>,
}

#[derive(Deserialize)]
struct TeamWithStates {
    #[serde(default)]
    states: Nodes<RawWorkflowState>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct IssueUpdateData {
    issue_update: MutationSuccess,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct CommentCreateData {
    comment_create: MutationSuccess,
}

#[derive(Deserialize, Debug)]
struct MutationSuccess {
    #[serde(default)]
    success: bool,
}
