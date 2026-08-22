//! Persisted application settings.
//!
//! Everything the user configures lives here: how to reach Flux, which agent
//! backends are available, which backend plays which role, and where each Flux
//! project lives on disk.

use crate::detect::ModelHost;
use crate::flux::FluxConfig;
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
            RunnerKind::Custom { command, .. } => command,
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
        }
    }
}

/// Links a Flux project to a checkout on this machine and the settings used to
/// work it. A project without a binding is visible in the UI but cannot be run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectBinding {
    pub project_id: String,
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

/// The whole persisted configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    #[serde(default)]
    pub flux: FluxConfig,
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
}

fn default_hosts() -> Vec<ModelHost> {
    vec![ModelHost::localhost()]
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            flux: FluxConfig::default(),
            profiles: Vec::new(),
            roles: BTreeMap::new(),
            bindings: Vec::new(),
            hosts: default_hosts(),
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
            autonomous: true,
        };

        let mut roles = BTreeMap::new();
        roles.insert(Role::Orchestrator, claude.id.clone());
        roles.insert(Role::Implementer, qwen.id.clone());
        roles.insert(Role::Reviewer, claude.id.clone());
        roles.insert(Role::Documenter, qwen.id.clone());

        Self {
            flux: FluxConfig::default(),
            profiles: vec![claude, qwen],
            roles,
            bindings: Vec::new(),
            hosts: default_hosts(),
        }
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
