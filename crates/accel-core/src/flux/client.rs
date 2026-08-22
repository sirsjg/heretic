//! REST client for the Flux API.

use crate::model::{Epic, Project, Task, TaskComment, TaskStatus};
use serde::Serialize;
use serde_json::json;
use std::collections::BTreeMap;

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

    /// The identity proxy in front of Flux answered instead of Flux — almost
    /// always a sign-in page, because this client is not authenticated with it.
    #[error("{0}")]
    ProxyChallenge(String),
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

    /// True when the proxy in front of Flux rejected us, rather than Flux itself.
    ///
    /// Worth distinguishing: the fix is a proxy credential, not a Flux API key.
    pub fn is_proxy_challenge(&self) -> bool {
        matches!(self, FluxError::ProxyChallenge(_))
    }

    pub fn is_not_found(&self) -> bool {
        matches!(self, FluxError::Api { status: 404, .. })
    }
}

pub type Result<T> = std::result::Result<T, FluxError>;

/// Connection settings for a Flux server.
///
/// A Flux server published on a domain usually sits behind an identity proxy
/// (Cloudflare Access, oauth2-proxy, Authelia, Pomerium and friends). That proxy
/// authenticates the request *before* Flux ever sees it, so Accelerate has to
/// satisfy two layers: the proxy's credential, and then Flux's own API key.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct FluxConfig {
    /// Base URL, e.g. `http://localhost:3000` or `https://flux.example.com`.
    pub base_url: String,
    /// Optional API key. Flux servers are locked by default, so this is normally set.
    ///
    /// Sent as `Authorization: Bearer …` unless [`Self::headers`] already sets
    /// `Authorization`, since the header can only carry one credential.
    #[serde(default)]
    pub api_key: Option<String>,
    /// Extra headers sent on every request, for the proxy in front of Flux.
    ///
    /// Cloudflare Access service tokens live here as `CF-Access-Client-Id` and
    /// `CF-Access-Client-Secret`.
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// Session cookie for proxies that authenticate with one, as it would appear
    /// in a `Cookie` header. Populated by signing in, or pasted by hand.
    #[serde(default)]
    pub cookie: Option<String>,
}

impl Default for FluxConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:3000".to_string(),
            api_key: None,
            headers: BTreeMap::new(),
            cookie: None,
        }
    }
}

impl FluxConfig {
    pub fn normalised_base(&self) -> String {
        self.base_url.trim_end_matches('/').to_string()
    }

    /// Whether the caller has claimed the `Authorization` header for the proxy,
    /// which means Flux's own API key cannot also be sent on it.
    pub fn authorization_taken_by_proxy(&self) -> bool {
        self.headers
            .keys()
            .any(|name| name.eq_ignore_ascii_case("authorization"))
    }

    /// Problems worth telling the user about before they hit a confusing 401.
    pub fn access_warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        if self.authorization_taken_by_proxy()
            && self.api_key.as_deref().is_some_and(|k| !k.is_empty())
        {
            warnings.push(
                "Your proxy uses the Authorization header, so Flux's own API key cannot be sent as well. \
Run Flux with FLUX_ALLOW_ANONYMOUS=1 behind the proxy and clear the API key, or move the proxy onto a \
different header."
                    .to_string(),
            );
        }

        if self.base_url.starts_with("http://")
            && !self.base_url.contains("localhost")
            && !self.base_url.contains("127.0.0.1")
            && (self.cookie.is_some() || !self.headers.is_empty())
        {
            warnings.push(
                "This server is plain HTTP, so proxy credentials travel unencrypted. Use HTTPS."
                    .to_string(),
            );
        }

        warnings
    }
}

/// What Flux says about the credentials it just saw.
///
/// Worth asking for explicitly: a keyless request to a server that requires one
/// still answers `200` with the *public* projects, which for a private board is
/// an empty list. Left undiagnosed that reads as "connected, no work to do"
/// rather than "you never authenticated".
#[derive(Debug, Clone, serde::Deserialize)]
pub struct AuthStatus {
    #[serde(default)]
    pub authenticated: bool,
    #[serde(default)]
    pub key_type: Option<String>,
    /// Whether this server rejects anonymous access.
    #[serde(default, rename = "authRequired")]
    pub auth_required: bool,
}

impl AuthStatus {
    /// True when the server wants a key and we did not present a valid one.
    pub fn needs_key(&self) -> bool {
        self.auth_required && !self.authenticated
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

        // Flux's own key, unless the proxy has claimed the Authorization header
        // for itself — only one credential fits on it.
        if !self.config.authorization_taken_by_proxy() {
            if let Some(key) = self.config.api_key.as_deref().filter(|k| !k.is_empty()) {
                builder = builder.bearer_auth(key);
            }
        }

        for (name, value) in &self.config.headers {
            builder = builder.header(name, value);
        }

        if let Some(cookie) = self.config.cookie.as_deref().filter(|c| !c.is_empty()) {
            builder = builder.header(reqwest::header::COOKIE, cookie);
        }

        builder
    }

    async fn send<T: serde::de::DeserializeOwned>(
        &self,
        builder: reqwest::RequestBuilder,
    ) -> Result<T> {
        let response = builder.send().await?;
        let status = response.status();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();
        let body = response.text().await?;

        // A proxy that answers instead of Flux is the common failure on a
        // published server, and its 200-with-a-login-page looks nothing like an
        // API error. Name it before trying to parse JSON out of HTML.
        if let Some(message) = detect_proxy_challenge(status.as_u16(), &content_type, &body) {
            return Err(FluxError::ProxyChallenge(message));
        }

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

    /// Ask Flux whether it accepted our credentials.
    pub async fn auth_status(&self) -> Result<AuthStatus> {
        self.send(self.request(reqwest::Method::GET, "/api/auth/status"))
            .await
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

/// Decide whether a response came from an identity proxy rather than Flux.
///
/// Two shapes matter. A proxy may reject outright (401/403 with an HTML body),
/// or it may redirect to its identity provider — which the HTTP client follows,
/// leaving us holding a sign-in page with a `200`. Both would otherwise surface
/// as an unintelligible JSON parse error.
pub(crate) fn detect_proxy_challenge(
    status: u16,
    content_type: &str,
    body: &str,
) -> Option<String> {
    let looks_like_html = content_type.contains("text/html") || {
        let start = body.trim_start().to_ascii_lowercase();
        start.starts_with("<!doctype html") || start.starts_with("<html")
    };

    if !looks_like_html {
        return None;
    }

    let haystack = body.to_ascii_lowercase();
    let provider = if haystack.contains("cloudflare access") || haystack.contains("cf-access") {
        Some("Cloudflare Access")
    } else if haystack.contains("oauth2-proxy") {
        Some("oauth2-proxy")
    } else if haystack.contains("authelia") {
        Some("Authelia")
    } else if haystack.contains("pomerium") {
        Some("Pomerium")
    } else {
        None
    };

    let who = provider.unwrap_or("The proxy in front of Flux");
    Some(format!(
        "{who} answered with a sign-in page instead of Flux (HTTP {status}). Accelerate is not \
authenticated with it. Sign in under Settings → Access, or add a service token."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with_headers(pairs: &[(&str, &str)]) -> FluxConfig {
        FluxConfig {
            headers: pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            ..FluxConfig::default()
        }
    }

    #[test]
    fn a_cloudflare_service_token_does_not_claim_authorization() {
        let config = config_with_headers(&[
            ("CF-Access-Client-Id", "abc.access"),
            ("CF-Access-Client-Secret", "shh"),
        ]);
        // Flux's own key is still free to travel on Authorization.
        assert!(!config.authorization_taken_by_proxy());
    }

    #[test]
    fn a_proxy_bearer_token_claims_authorization_whatever_its_casing() {
        assert!(
            config_with_headers(&[("Authorization", "Bearer jwt")]).authorization_taken_by_proxy()
        );
        assert!(
            config_with_headers(&[("authorization", "Bearer jwt")]).authorization_taken_by_proxy()
        );
        assert!(config_with_headers(&[("AUTHORIZATION", "Basic x")]).authorization_taken_by_proxy());
    }

    #[test]
    fn clashing_credentials_are_flagged_before_the_user_hits_a_401() {
        let mut config = config_with_headers(&[("Authorization", "Bearer jwt")]);
        config.api_key = Some("flx_key".into());

        let warnings = config.access_warnings();
        assert!(
            warnings.iter().any(|w| w.contains("FLUX_ALLOW_ANONYMOUS")),
            "expected advice about the clash, got {warnings:?}"
        );
    }

    #[test]
    fn no_clash_means_no_warning() {
        let mut config = config_with_headers(&[("CF-Access-Client-Id", "abc")]);
        config.api_key = Some("flx_key".into());
        config.base_url = "https://flux.example.com".into();
        assert!(config.access_warnings().is_empty());
    }

    #[test]
    fn sending_proxy_credentials_over_plain_http_is_called_out() {
        let mut config = config_with_headers(&[("CF-Access-Client-Id", "abc")]);
        config.base_url = "http://flux.example.com".into();
        assert!(config.access_warnings().iter().any(|w| w.contains("HTTPS")));
    }

    #[test]
    fn a_local_server_is_not_nagged_about_https() {
        let mut config = config_with_headers(&[("X-Token", "abc")]);
        config.base_url = "http://localhost:3000".into();
        assert!(config.access_warnings().is_empty());
    }

    #[test]
    fn a_login_page_returned_as_success_is_recognised() {
        let message = detect_proxy_challenge(
            200,
            "text/html; charset=utf-8",
            "<!DOCTYPE html><html><body>Sign in to continue</body></html>",
        );
        assert!(message.is_some());
        assert!(message.unwrap().contains("sign-in page"));
    }

    #[test]
    fn known_providers_are_named_so_the_user_knows_what_to_configure() {
        let cloudflare = detect_proxy_challenge(
            403,
            "text/html",
            "<html><body>Cloudflare Access denied</body></html>",
        )
        .unwrap();
        assert!(cloudflare.contains("Cloudflare Access"));

        let authelia =
            detect_proxy_challenge(401, "text/html", "<html>authelia portal</html>").unwrap();
        assert!(authelia.contains("Authelia"));
    }

    #[test]
    fn html_is_detected_even_without_a_content_type() {
        assert!(detect_proxy_challenge(200, "", "  <html><head></head></html>").is_some());
    }

    #[test]
    fn a_genuine_flux_response_is_left_alone() {
        assert!(detect_proxy_challenge(200, "application/json", r#"{"projects":[]}"#).is_none());
        // Flux's own 401 is an API error, not a proxy challenge.
        assert!(
            detect_proxy_challenge(401, "application/json", r#"{"error":"Unauthorized"}"#)
                .is_none()
        );
    }
}

#[cfg(test)]
mod auth_status_tests {
    use super::*;

    fn parse(json: &str) -> AuthStatus {
        serde_json::from_str(json).expect("auth status should parse")
    }

    #[test]
    fn a_keyless_request_to_a_locked_server_is_recognised() {
        // Exactly what a published Flux server answers when no key is presented.
        let status = parse(r#"{"authenticated":false,"keyType":"anonymous","authRequired":true}"#);
        assert!(status.needs_key());
    }

    #[test]
    fn an_authenticated_request_needs_nothing_more() {
        let status = parse(r#"{"authenticated":true,"keyType":"server","authRequired":true}"#);
        assert!(!status.needs_key());
    }

    #[test]
    fn an_open_server_never_asks_for_a_key() {
        let status = parse(r#"{"authenticated":false,"keyType":"anonymous","authRequired":false}"#);
        assert!(!status.needs_key());
    }

    #[test]
    fn an_older_server_that_omits_the_field_is_not_nagged() {
        // Forward/backward compatibility: absent means "not required".
        let status = parse(r#"{"authenticated":false}"#);
        assert!(!status.needs_key());
    }
}
