//! REST client for the Flux API.

use crate::model::{Epic, Project, Task, TaskComment, TaskStatus};
use serde::Serialize;
use serde_json::json;

/// Errors surfaced by the Flux client.
#[derive(Debug, thiserror::Error)]
pub enum FluxError {
    #[error("flux request failed: {0}")]
    Transport(#[from] reqwest::Error),

    /// Flux answered, but not with success. `status` 401 usually means the server
    /// is locked (no `FLUX_API_KEY` configured here) rather than a bad key.
    #[error("flux api error (status {status}): {message}")]
    Api { status: u16, message: String },

    #[error("flux returned a response Accelerate could not parse: {0}")]
    Decode(String),
}

impl FluxError {
    /// True when the failure is an authentication problem the user can fix by
    /// entering an API key in Settings.
    pub fn is_auth(&self) -> bool {
        matches!(
            self,
            FluxError::Api {
                status: 401 | 403,
                ..
            }
        )
    }

    pub fn is_not_found(&self) -> bool {
        matches!(self, FluxError::Api { status: 404, .. })
    }
}

pub type Result<T> = std::result::Result<T, FluxError>;

/// Connection settings for a Flux server.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct FluxConfig {
    /// Base URL, e.g. `http://localhost:3000`.
    pub base_url: String,
    /// Optional API key. Flux servers are locked by default, so this is normally set.
    #[serde(default)]
    pub api_key: Option<String>,
}

impl Default for FluxConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:3000".to_string(),
            api_key: None,
        }
    }
}

impl FluxConfig {
    pub fn normalised_base(&self) -> String {
        self.base_url.trim_end_matches('/').to_string()
    }
}

/// An async client for the Flux REST API.
#[derive(Debug, Clone)]
pub struct FluxClient {
    config: FluxConfig,
    http: reqwest::Client,
}

impl FluxClient {
    pub fn new(config: FluxConfig) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
        Ok(Self { config, http })
    }

    pub fn config(&self) -> &FluxConfig {
        &self.config
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.config.normalised_base(), path)
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let mut builder = self.http.request(method, self.url(path));
        if let Some(key) = self.config.api_key.as_deref().filter(|k| !k.is_empty()) {
            builder = builder.bearer_auth(key);
        }
        builder
    }

    async fn send<T: serde::de::DeserializeOwned>(
        &self,
        builder: reqwest::RequestBuilder,
    ) -> Result<T> {
        let response = builder.send().await?;
        let status = response.status();
        let body = response.text().await?;

        if !status.is_success() {
            // Flux errors are `{ "error": "..." }`; fall back to the raw body.
            let message = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string))
                .filter(|m| !m.is_empty())
                .unwrap_or_else(|| {
                    let trimmed = body.trim();
                    if trimmed.is_empty() {
                        status
                            .canonical_reason()
                            .unwrap_or("unknown error")
                            .to_string()
                    } else {
                        trimmed.to_string()
                    }
                });
            return Err(FluxError::Api {
                status: status.as_u16(),
                message,
            });
        }

        serde_json::from_str(&body).map_err(|e| FluxError::Decode(format!("{e}: {body}")))
    }

    /// Cheap reachability probe used by the connection indicator in the UI.
    pub async fn health(&self) -> Result<bool> {
        let response = self.request(reqwest::Method::GET, "/health").send().await?;
        Ok(response.status().is_success())
    }

    pub async fn list_projects(&self) -> Result<Vec<Project>> {
        self.send(self.request(reqwest::Method::GET, "/api/projects"))
            .await
    }

    pub async fn get_project(&self, project_id: &str) -> Result<Project> {
        self.send(self.request(reqwest::Method::GET, &format!("/api/projects/{project_id}")))
            .await
    }

    pub async fn list_epics(&self, project_id: &str) -> Result<Vec<Epic>> {
        self.send(self.request(
            reqwest::Method::GET,
            &format!("/api/projects/{project_id}/epics"),
        ))
        .await
    }

    pub async fn get_epic(&self, epic_id: &str) -> Result<Epic> {
        self.send(self.request(reqwest::Method::GET, &format!("/api/epics/{epic_id}")))
            .await
    }

    /// Flip an epic's `auto` flag — the switch that decides whether Accelerate may
    /// work its tasks unattended.
    pub async fn set_epic_auto(&self, epic_id: &str, auto: bool) -> Result<Epic> {
        self.send(
            self.request(reqwest::Method::PATCH, &format!("/api/epics/{epic_id}"))
                .json(&json!({ "auto": auto })),
        )
        .await
    }

    pub async fn list_tasks(&self, project_id: &str) -> Result<Vec<Task>> {
        self.send(self.request(
            reqwest::Method::GET,
            &format!("/api/projects/{project_id}/tasks"),
        ))
        .await
    }

    pub async fn get_task(&self, task_id: &str) -> Result<Task> {
        self.send(self.request(reqwest::Method::GET, &format!("/api/tasks/{task_id}")))
            .await
    }

    /// Move a task to a new status, honouring Flux's planning -> todo -> in_progress
    /// rule by walking intermediate statuses when needed.
    ///
    /// `agent_name` is what shows up on the Flux board as the worker badge.
    pub async fn move_task_status(
        &self,
        task_id: &str,
        target: TaskStatus,
        agent_name: Option<&str>,
    ) -> Result<Task> {
        let current = self.get_task(task_id).await?;
        let from = current.status_enum().unwrap_or(TaskStatus::Todo);

        let mut latest = current;
        for step in from.path_to(target) {
            let mut body = json!({ "status": step.as_str() });
            if let Some(name) = agent_name {
                body["agent_name"] = json!(name);
            }
            latest = self
                .send(
                    self.request(reqwest::Method::PATCH, &format!("/api/tasks/{task_id}"))
                        .json(&body),
                )
                .await?;
        }
        Ok(latest)
    }

    /// Record an external blocker on a task, or clear it by passing `None`.
    pub async fn set_blocked_reason(&self, task_id: &str, reason: Option<&str>) -> Result<Task> {
        self.send(
            self.request(reqwest::Method::PATCH, &format!("/api/tasks/{task_id}"))
                .json(&json!({ "blocked_reason": reason })),
        )
        .await
    }

    /// Append a comment. Comments are how agents leave memory on the board, so
    /// Accelerate writes one at every meaningful step of a run.
    pub async fn add_comment(
        &self,
        task_id: &str,
        body: &str,
        agent_name: Option<&str>,
    ) -> Result<TaskComment> {
        let mut payload = json!({ "body": body, "author": "mcp" });
        if let Some(name) = agent_name {
            payload["agent_name"] = json!(name);
        }
        self.send(
            self.request(
                reqwest::Method::POST,
                &format!("/api/tasks/{task_id}/comments"),
            )
            .json(&payload),
        )
        .await
    }
}
