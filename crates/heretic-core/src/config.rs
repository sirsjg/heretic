//! Persisted application settings.
//!
//! Everything the user configures lives here: how to reach Flux, which agent
//! backends are available, which backend plays which role, and where each Flux
//! project lives on disk.

use crate::detect::ModelHost;
use crate::flux::FluxConfig;
use crate::linear::LinearConfig;
use crate::model::SourceKind;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The job an agent is doing in a run. Each role is bound to a model profile, so
/// a cheap local model can implement while a stronger one reviews (or the reverse).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// Reads the task and writes the brief the implementer works from.
    Orchestrator,
    /// Does the actual work in the repository.
    Implementer,
    /// Reads the resulting diff and returns a verdict.
    Reviewer,
    /// Updates docs/changelogs once the work is approved.
    Documenter,
}

impl Role {
    pub const ALL: [Role; 4] = [
        Role::Orchestrator,
        Role::Implementer,
        Role::Reviewer,
        Role::Documenter,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Orchestrator => "orchestrator",
            Role::Implementer => "implementer",
            Role::Reviewer => "reviewer",
            Role::Documenter => "documenter",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Role::Orchestrator => "Orchestrator",
            Role::Implementer => "Implementer",
            Role::Reviewer => "Reviewer",
            Role::Documenter => "Documenter",
        }
    }
}

/// Which CLI actually runs, and how it is invoked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunnerKind {
    /// Anthropic's Claude Code CLI.
    ClaudeCode,
    /// OpenAI's Codex CLI against a hosted model.
    Codex,
    /// Codex CLI in open-model mode (`--oss`), which drives a local Ollama server.
    CodexOss {
        /// Ollama endpoint. Codex defaults to `http://localhost:11434/v1`.
        #[serde(default)]
        base_url: Option<String>,
    },
    /// The OpenCode CLI.
    ///
    /// One variant covers both cases, because OpenCode has no separate
    /// local-model mode: with no `base_url` it uses whatever providers its own
    /// configuration sets up, and with one it is handed a provider pointing at
    /// that host.
    ///
    /// Named explicitly: `rename_all` would spell this `open_code`, which is
    /// neither what the CLI is called nor what the UI sends.
    #[serde(rename = "opencode")]
    OpenCode {
        /// An OpenAI-compatible endpoint to drive instead of OpenCode's own
        /// providers. `None` leaves the user's OpenCode configuration alone.
        #[serde(default)]
        base_url: Option<String>,
    },
    /// Any other agent CLI, described by a command template.
    ///
    /// `{{prompt}}` in an argument is replaced with the generated prompt; if no
    /// argument contains it, the prompt is written to the process's stdin instead.
    /// `{{model}}` is replaced with the profile's model id.
    Custom {
        command: String,
        #[serde(default)]
        args: Vec<String>,
    },
}

impl RunnerKind {
    pub fn display_name(&self) -> &str {
        match self {
            RunnerKind::ClaudeCode => "Claude Code",
            RunnerKind::Codex => "Codex",
            RunnerKind::CodexOss { .. } => "Codex (local)",
            RunnerKind::OpenCode { base_url: None } => "OpenCode",
            RunnerKind::OpenCode { .. } => "OpenCode (local)",
            RunnerKind::Custom { command, .. } => command,
        }
    }
}

/// How hard a model should think before acting.
///
/// Backends spell this differently — a thinking-token budget for Claude Code, a
/// named effort for Codex and OpenCode — so the profile stores the intent and
/// the command builder translates it. `None` leaves the backend's default
/// untouched, which is the right choice for models that do not reason at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningEffort {
    Low,
    Medium,
    High,
}

impl ReasoningEffort {
    pub fn as_str(self) -> &'static str {
        match self {
            ReasoningEffort::Low => "low",
            ReasoningEffort::Medium => "medium",
            ReasoningEffort::High => "high",
        }
    }

    /// The thinking-token budget for backends that take one, matching Claude
    /// Code's own tiers ("think", "megathink", "ultrathink").
    ///
    /// Verified against Claude Code 2.1, which reads `MAX_THINKING_TOKENS`
    /// from the environment. If a later CLI re-tiers these, only this mapping
    /// needs to move.
    pub fn thinking_tokens(self) -> u32 {
        match self {
            ReasoningEffort::Low => 4_000,
            ReasoningEffort::Medium => 10_000,
            ReasoningEffort::High => 31_999,
        }
    }
}

/// A configured, selectable agent: a runner plus the model it should use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelProfile {
    pub id: String,
    /// What the user sees, e.g. "Qwen3 Coder 30B (local)".
    pub name: String,
    pub runner: RunnerKind,
    /// Model identifier passed to the runner. `None` means the CLI's own default.
    #[serde(default)]
    pub model: Option<String>,
    /// Extra CLI arguments appended verbatim.
    #[serde(default)]
    pub extra_args: Vec<String>,
    /// Extra environment variables for the process.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Hard ceiling on a single run, in seconds. `None` means no timeout.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// The model's context window, in tokens, when the host reports one.
    ///
    /// Passed to backends that would otherwise guess.
    #[serde(default)]
    pub context_window: Option<u64>,
    /// How hard the model should think, for backends that let us say.
    /// `None` keeps the backend's own default.
    #[serde(default)]
    pub reasoning_effort: Option<ReasoningEffort>,
    /// Whether this profile may take actions without per-action approval.
    /// Local sandboxed models are typically trusted; anything touching a real
    /// repo unattended has to be, or it will hang on a permission prompt.
    #[serde(default = "default_true")]
    pub autonomous: bool,
}

fn default_true() -> bool {
    true
}

impl ModelProfile {
    /// The name written to the Flux board as the worker badge.
    pub fn agent_name(&self, role: Role) -> String {
        format!("{} · {}", self.name, role.label())
    }
}

/// Where agents run when several work the same project at once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Isolation {
    /// Each task gets its own `git worktree` and branch. Safe for parallel work.
    #[default]
    Worktree,
    /// Agents edit the checkout directly. Heretic serialises runs in this mode.
    InPlace,
}

/// What happens to a worktree branch once a run finishes cleanly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Integration {
    /// Leave the branch and worktree in place for the user to inspect.
    #[default]
    Leave,
    /// Merge the branch back into the base branch, then remove the worktree.
    Merge,
}

/// The stage sequence a task run goes through.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pipeline {
    /// Produce an implementation brief before touching code.
    #[serde(default)]
    pub plan: bool,
    /// Review the diff and gate completion on the verdict.
    #[serde(default = "default_true")]
    pub review: bool,
    /// Run a documentation pass after a successful review.
    #[serde(default)]
    pub document: bool,
    /// How many times a "changes requested" verdict may send work back to the
    /// implementer before the run is handed to a human.
    #[serde(default = "default_max_revisions")]
    pub max_revisions: u32,
    /// Yolo mode: agents never stop to ask the user anything.
    ///
    /// On by default — it is what unattended runs need. Switched off, an agent
    /// that is genuinely blocked may end its turn with a question; the run then
    /// waits for the user's answer before that stage is run again.
    #[serde(default = "default_true")]
    pub yolo: bool,
}

fn default_max_revisions() -> u32 {
    2
}

impl Default for Pipeline {
    fn default() -> Self {
        Self {
            plan: false,
            review: true,
            document: false,
            max_revisions: default_max_revisions(),
            yolo: true,
        }
    }
}

/// Links a Flux project to a checkout on this machine and the settings used to
/// work it. A project without a binding is visible in the UI but cannot be run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectBinding {
    pub project_id: String,
    /// Which tracker the project lives on. Bindings saved before other
    /// sources existed carry no field and stay Flux.
    #[serde(default)]
    pub source: SourceKind,
    /// Absolute path to the git repository.
    pub repo_path: PathBuf,
    /// Branch new work forks from. `None` means the repository's current HEAD.
    #[serde(default)]
    pub base_branch: Option<String>,
    #[serde(default)]
    pub isolation: Isolation,
    #[serde(default)]
    pub integration: Integration,
    #[serde(default)]
    pub pipeline: Pipeline,
    /// Role bindings for this project; falls back to the global assignments.
    #[serde(default)]
    pub roles: BTreeMap<Role, String>,
    /// Whether Heretic may pick up auto-enabled work here without being asked.
    #[serde(default)]
    pub auto_run: bool,
    /// How many task runs may be in flight for this project at once.
    #[serde(default = "default_concurrency")]
    pub max_parallel: u32,
}

fn default_concurrency() -> u32 {
    2
}

impl ProjectBinding {
    pub fn new(project_id: impl Into<String>, repo_path: impl Into<PathBuf>) -> Self {
        Self {
            project_id: project_id.into(),
            source: SourceKind::default(),
            repo_path: repo_path.into(),
            base_branch: None,
            isolation: Isolation::default(),
            integration: Integration::default(),
            pipeline: Pipeline::default(),
            roles: BTreeMap::new(),
            auto_run: false,
            max_parallel: default_concurrency(),
        }
    }

    /// Runs may only overlap when each has its own worktree.
    pub fn effective_parallelism(&self) -> u32 {
        match self.isolation {
            Isolation::Worktree => self.max_parallel.max(1),
            Isolation::InPlace => 1,
        }
    }
}

/// Remote access: a small web server inside Heretic that serves this same
/// interface to a phone or another machine.
///
/// Off by default, and bound to this machine only until the user says
/// otherwise — a listener is a door, and a door should be opened on purpose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RemoteConfig {
    pub enabled: bool,
    /// The interface to listen on. `127.0.0.1` keeps it to this machine,
    /// `0.0.0.0` opens every interface, and a specific address — a Tailscale
    /// one, say — just that one.
    pub bind: String,
    pub port: u16,
    /// The bearer token a remote client must present. Generated the first
    /// time remote access is switched on; rotating it logs every phone out.
    pub token: Option<String>,
    /// Where this server is reached from outside, when that is not simply
    /// `http://<address>:<port>` — behind Tailscale Serve or a reverse proxy.
    /// Used for the pairing link and for links in notifications.
    pub public_url: Option<String>,
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind: "127.0.0.1".into(),
            port: 7411,
            token: None,
            public_url: None,
        }
    }
}

impl RemoteConfig {
    /// A fresh bearer token: 256 bits from the OS, as hex.
    ///
    /// Two v4 UUIDs rather than a new dependency — each carries 122 random
    /// bits, and the `uuid` crate draws them from the operating system.
    pub fn generate_token() -> String {
        format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        )
    }

    /// The address clients should use, preferring what the user told us.
    pub fn base_url_for(&self, host: &str) -> String {
        match self.public_url.as_deref().map(str::trim) {
            Some(url) if !url.is_empty() => url.trim_end_matches('/').to_string(),
            _ => {
                let host = if host.contains(':') && !host.starts_with('[') {
                    format!("[{host}]")
                } else {
                    host.to_string()
                };
                format!("http://{host}:{}", self.port)
            }
        }
    }
}

/// Push notifications for the moments a run needs a person: a question, a
/// failure, work sitting on a branch waiting to be merged.
///
/// Both services are plain HTTP posts with a phone app on the other end;
/// ntfy is also self-hostable. Neither is contacted unless it is configured.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct NotifyConfig {
    pub ntfy: Option<NtfyConfig>,
    pub pushover: Option<PushoverConfig>,
    /// Also say when a run finishes cleanly. Off by default: on a busy board
    /// that is a buzz every few minutes, and none of them need anything.
    pub on_success: bool,
}

impl NotifyConfig {
    /// Whether any service is set up well enough to be posted to.
    pub fn enabled(&self) -> bool {
        self.ntfy.as_ref().is_some_and(NtfyConfig::enabled)
            || self.pushover.as_ref().is_some_and(PushoverConfig::enabled)
    }
}

/// An [ntfy](https://ntfy.sh) topic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NtfyConfig {
    /// The server, `https://ntfy.sh` unless self-hosted.
    pub server: String,
    /// The topic to publish to. Anyone who knows it can subscribe, so make it
    /// unguessable on the public server.
    pub topic: String,
    /// An access token, for a protected topic.
    pub token: Option<String>,
}

impl Default for NtfyConfig {
    fn default() -> Self {
        Self {
            server: "https://ntfy.sh".into(),
            topic: String::new(),
            token: None,
        }
    }
}

impl NtfyConfig {
    pub fn enabled(&self) -> bool {
        !self.topic.trim().is_empty() && !self.server.trim().is_empty()
    }
}

/// A [Pushover](https://pushover.net) application and the user it delivers to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PushoverConfig {
    pub user_key: String,
    pub app_token: String,
}

impl PushoverConfig {
    pub fn enabled(&self) -> bool {
        !self.user_key.trim().is_empty() && !self.app_token.trim().is_empty()
    }
}

/// The whole persisted configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    #[serde(default)]
    pub flux: FluxConfig,
    /// The Linear connection, when one is configured. One API key covers a
    /// whole workspace, so a single entry is enough.
    #[serde(default)]
    pub linear: Option<LinearConfig>,
    #[serde(default)]
    pub profiles: Vec<ModelProfile>,
    /// Default role -> profile id bindings, used when a project does not override.
    #[serde(default)]
    pub roles: BTreeMap<Role, String>,
    #[serde(default)]
    pub bindings: Vec<ProjectBinding>,
    /// Machines serving models — this laptop's Ollama, and any box on the
    /// network worth scanning for weights.
    #[serde(default = "default_hosts")]
    pub hosts: Vec<ModelHost>,
    /// Serving this interface to a phone or another machine.
    #[serde(default)]
    pub remote: RemoteConfig,
    /// Push notifications for the moments a run needs a person.
    #[serde(default)]
    pub notifications: NotifyConfig,
}

fn default_hosts() -> Vec<ModelHost> {
    vec![ModelHost::localhost()]
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            flux: FluxConfig::default(),
            linear: None,
            profiles: Vec::new(),
            roles: BTreeMap::new(),
            bindings: Vec::new(),
            hosts: default_hosts(),
            remote: RemoteConfig::default(),
            notifications: NotifyConfig::default(),
        }
    }
}

impl Settings {
    /// A first-run configuration: Claude Code for judgement-heavy roles and a
    /// local Qwen through Codex's open-model mode for the implementation grind.
    pub fn with_starter_profiles() -> Self {
        let claude = ModelProfile {
            id: "claude-code".into(),
            name: "Claude Code".into(),
            runner: RunnerKind::ClaudeCode,
            model: None,
            extra_args: Vec::new(),
            env: BTreeMap::new(),
            timeout_secs: Some(3600),
            context_window: None,
            reasoning_effort: None,
            autonomous: true,
        };
        let qwen = ModelProfile {
            id: "qwen-local".into(),
            name: "Qwen3 Coder (local)".into(),
            runner: RunnerKind::CodexOss { base_url: None },
            model: Some("qwen3-coder:30b".into()),
            extra_args: Vec::new(),
            env: BTreeMap::new(),
            timeout_secs: Some(5400),
            context_window: None,
            reasoning_effort: None,
            autonomous: true,
        };

        let mut roles = BTreeMap::new();
        roles.insert(Role::Orchestrator, claude.id.clone());
        roles.insert(Role::Implementer, qwen.id.clone());
        roles.insert(Role::Reviewer, claude.id.clone());
        roles.insert(Role::Documenter, qwen.id.clone());

        Self {
            flux: FluxConfig::default(),
            linear: None,
            profiles: vec![claude, qwen],
            roles,
            bindings: Vec::new(),
            hosts: default_hosts(),
            remote: RemoteConfig::default(),
            notifications: NotifyConfig::default(),
        }
    }

    /// Whether a Linear workspace is connected.
    pub fn linear_enabled(&self) -> bool {
        self.linear.as_ref().is_some_and(LinearConfig::enabled)
    }

    /// Insert or replace a model host.
    pub fn upsert_host(&mut self, host: ModelHost) {
        match self.hosts.iter_mut().find(|h| h.id == host.id) {
            Some(existing) => *existing = host,
            None => self.hosts.push(host),
        }
    }

    pub fn remove_host(&mut self, host_id: &str) {
        self.hosts.retain(|host| host.id != host_id);
    }

    pub fn profile(&self, id: &str) -> Option<&ModelProfile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    pub fn binding(&self, project_id: &str) -> Option<&ProjectBinding> {
        self.bindings.iter().find(|b| b.project_id == project_id)
    }

    pub fn binding_mut(&mut self, project_id: &str) -> Option<&mut ProjectBinding> {
        self.bindings
            .iter_mut()
            .find(|b| b.project_id == project_id)
    }

    /// Insert or replace a project binding.
    pub fn upsert_binding(&mut self, binding: ProjectBinding) {
        match self.binding_mut(&binding.project_id) {
            Some(existing) => *existing = binding,
            None => self.bindings.push(binding),
        }
    }

    /// Resolve the profile for a role, preferring the project's own binding.
    pub fn resolve_profile(&self, project_id: &str, role: Role) -> Option<&ModelProfile> {
        let project_choice = self
            .binding(project_id)
            .and_then(|b| b.roles.get(&role))
            .and_then(|id| self.profile(id));
        project_choice.or_else(|| self.roles.get(&role).and_then(|id| self.profile(id)))
    }

    /// Problems that would stop a run, phrased for display in the UI.
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();

        if self.flux.base_url.trim().is_empty() {
            problems.push("Flux server URL is not set.".into());
        }
        if self.profiles.is_empty() {
            problems.push("No model profiles are configured.".into());
        }

        for (role, profile_id) in &self.roles {
            if self.profile(profile_id).is_none() {
                problems.push(format!(
                    "The {} role points at a model profile that no longer exists ({profile_id}).",
                    role.label()
                ));
            }
        }

        for binding in &self.bindings {
            if !binding.repo_path.is_absolute() {
                problems.push(format!(
                    "Project {} needs an absolute repository path.",
                    binding.project_id
                ));
            }
            for (role, profile_id) in &binding.roles {
                if self.profile(profile_id).is_none() {
                    problems.push(format!(
                        "Project {} binds the {} role to a missing profile ({profile_id}).",
                        binding.project_id,
                        role.label()
                    ));
                }
            }
        }

        problems
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_saved_before_remote_access_existed_still_load() {
        let settings: Settings =
            serde_json::from_str(r#"{"flux":{"base_url":"http://localhost:3000"}}"#)
                .expect("old settings should parse");
        assert!(!settings.remote.enabled);
        assert_eq!(settings.remote.bind, "127.0.0.1");
        assert_eq!(settings.remote.port, 7411);
        assert!(settings.remote.token.is_none());
        assert!(!settings.notifications.enabled());
    }

    #[test]
    fn a_generated_token_is_long_random_hex() {
        let a = RemoteConfig::generate_token();
        let b = RemoteConfig::generate_token();
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn the_public_url_wins_over_the_bound_address() {
        let mut remote = RemoteConfig::default();
        assert_eq!(remote.base_url_for("100.64.0.9"), "http://100.64.0.9:7411");
        assert_eq!(remote.base_url_for("fe80::1"), "http://[fe80::1]:7411");
        remote.public_url = Some("https://heretic.example.ts.net/".into());
        assert_eq!(
            remote.base_url_for("100.64.0.9"),
            "https://heretic.example.ts.net"
        );
    }

    #[test]
    fn notifications_count_as_enabled_only_with_a_usable_service() {
        let mut config = NotifyConfig::default();
        assert!(!config.enabled());
        config.ntfy = Some(NtfyConfig::default());
        assert!(!config.enabled(), "an empty topic is nowhere to post");
        config.ntfy = Some(NtfyConfig {
            topic: "heretic-abc".into(),
            ..NtfyConfig::default()
        });
        assert!(config.enabled());
    }

    /// The tag each runner is written under, in settings on disk and across the
    /// bridge to the UI. These strings are duplicated in `ui/src/lib/types.ts`,
    /// so a variant renamed on one side without the other is rejected by the
    /// command layer rather than by anything a user can act on.
    #[test]
    fn every_runner_keeps_the_tag_the_ui_sends() {
        let cases = [
            (RunnerKind::ClaudeCode, "claude_code"),
            (RunnerKind::Codex, "codex"),
            (RunnerKind::CodexOss { base_url: None }, "codex_oss"),
            (RunnerKind::OpenCode { base_url: None }, "opencode"),
            (
                RunnerKind::Custom {
                    command: "aider".into(),
                    args: Vec::new(),
                },
                "custom",
            ),
        ];

        for (runner, tag) in cases {
            let json = serde_json::to_value(&runner).expect("runner should serialise");
            assert_eq!(json["kind"], tag, "wrong tag for {runner:?}");

            let parsed: RunnerKind =
                serde_json::from_value(json).expect("runner should round-trip");
            assert_eq!(parsed, runner);
        }
    }

    #[test]
    fn a_runner_the_ui_sends_is_accepted_verbatim() {
        // Exactly what the UI puts on the wire when a host is bound.
        let runner: RunnerKind =
            serde_json::from_str(r#"{"kind":"opencode","base_url":"http://localhost:11434/v1"}"#)
                .expect("the UI's opencode runner should parse");
        assert_eq!(
            runner,
            RunnerKind::OpenCode {
                base_url: Some("http://localhost:11434/v1".into())
            }
        );
    }
}
