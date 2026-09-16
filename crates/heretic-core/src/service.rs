//! The operations every interface exposes, in one place.
//!
//! The desktop shell hands these to its webview as Tauri commands; the remote
//! server hands the same ones to a phone over HTTP. Neither adds behaviour of
//! its own, which is what keeps them interchangeable — and what keeps the
//! logic testable without a window or a socket.
//!
//! Every operation returns `Result<_, String>` because both shells surface the
//! error text directly to the person using them. The messages are therefore
//! written to be read by a person, not a developer.

use crate::config::{ProjectBinding, Settings};
use crate::detect::{self, CliStatus, HostProbe, ModelHost};
use crate::flux::{FluxClient, FluxEvent, FluxWatcher};
use crate::history::RunHistory;
use crate::model::{Epic, Project, SourceKind, Task};
use crate::notify;
use crate::orchestrator::{Engine, RunRecord, RunStage, RunStatus};
use crate::selection::BoardSnapshot;
use crate::source::TaskSource;
use crate::store::SettingsStore;
use crate::worktree::{Commit, FileChange};
use crate::LinearClient;
use serde::Serialize;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

pub type Response<T> = Result<T, String>;

/// How often the auto loop looks for newly-ready work.
///
/// Flux's event stream normally tells us sooner; this is the safety net for
/// changes made while disconnected, or by a CLI writing the data file directly.
pub const AUTO_POLL: Duration = Duration::from_secs(45);

/// A task plus whether Heretic may start it, and why not.
#[derive(Debug, Clone, Serialize)]
pub struct TaskView {
    pub task: Task,
    pub ineligible: Option<&'static str>,
}

/// Everything one board screen needs, in a single round trip.
#[derive(Debug, Clone, Serialize)]
pub struct BoardView {
    pub project: Project,
    pub epics: Vec<Epic>,
    pub tasks: Vec<TaskView>,
    /// Task ids that could be started now, most important first.
    pub ready: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConnectionState {
    pub connected: bool,
    pub error: Option<String>,
    /// What kind of problem it is, so the interface can point at the fix rather
    /// than showing one undifferentiated red dot.
    pub kind: &'static str,
    /// Configuration that will bite later, even when the connection works.
    pub warnings: Vec<String>,
}

/// Everything Heretic can find: agent CLIs on this machine, and the models
/// each configured host is holding.
#[derive(Debug, Clone, Serialize)]
pub struct Environment {
    pub clis: Vec<CliStatus>,
    pub hosts: Vec<HostProbe>,
    /// Which platform this is, so the interface can leave room for macOS
    /// window controls.
    pub os: &'static str,
}

/// One run, reduced to what fits on a wrist or a lens.
#[derive(Debug, Clone, Serialize)]
pub struct RunGlance {
    pub id: String,
    pub project_name: String,
    pub task_title: String,
    pub status: RunStatus,
    pub stage: RunStage,
    pub agent: Option<String>,
    pub branch: Option<String>,
    pub started_at: String,
    /// The question the run is paused on, when it is waiting.
    pub question: Option<String>,
}

/// The state of the board in a glance — what a ticker, a status bar or a
/// pair of glasses would show.
#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    /// Runs in flight, questions included.
    pub active: usize,
    /// Runs paused on a question.
    pub waiting: usize,
    /// Runs that ended needing a person.
    pub attention: usize,
    /// Finished runs whose work is still sitting on a branch.
    pub unmerged: usize,
    /// One line saying all of the above, e.g. `2 running · 1 waiting for you`.
    pub line: String,
    /// The runs that matter right now: active first, then anything needing a
    /// decision. Finished-and-dealt-with runs are left out.
    pub runs: Vec<RunGlance>,
}

/// Owns the engine, the settings on disk, and the Flux relay.
pub struct Service {
    pub engine: Arc<Engine>,
    pub store: SettingsStore,
    flux_events: broadcast::Sender<FluxEvent>,
}

impl Service {
    /// Load settings and past runs from disk, falling back to a starter
    /// configuration.
    ///
    /// A corrupt settings file must not stop the app from opening — the user
    /// needs the Settings screen to fix it.
    pub fn load() -> Self {
        let store = SettingsStore::default();
        let settings = match store.load() {
            Ok(settings) => settings,
            Err(error) => {
                tracing::error!(%error, "could not read settings; starting with defaults");
                Settings::with_starter_profiles()
            }
        };

        Self::new(
            Arc::new(Engine::with_history(settings, RunHistory::default())),
            store,
        )
    }

    pub fn new(engine: Arc<Engine>, store: SettingsStore) -> Self {
        let (flux_events, _) = broadcast::channel(256);
        Self {
            engine,
            store,
            flux_events,
        }
    }

    /// The engine's own events: runs changing, agents talking.
    pub fn subscribe(&self) -> broadcast::Receiver<crate::EngineEvent> {
        self.engine.subscribe()
    }

    /// What the Flux server announces on its live stream, relayed.
    pub fn subscribe_flux(&self) -> broadcast::Receiver<FluxEvent> {
        self.flux_events.subscribe()
    }

    /// Everything that runs for as long as the process does: the Flux watcher,
    /// the auto loop, and the notifier.
    ///
    /// Returned as a future rather than spawned, so the caller decides which
    /// runtime owns it — a desktop shell's setup hook does not necessarily run
    /// inside one.
    pub async fn run_background(self: Arc<Self>) {
        let auto = tokio::spawn(Arc::clone(&self.engine).auto_loop(AUTO_POLL));
        let notifier = tokio::spawn(notify::watch(Arc::clone(&self.engine)));
        let watcher = tokio::spawn(Arc::clone(&self).relay_flux());
        let _ = tokio::join!(auto, notifier, watcher);
    }

    /// Watch Flux so the board reflects changes made elsewhere, and so work
    /// switched to Auto in the Flux UI is picked up promptly. The watcher is
    /// rebuilt whenever the Flux settings change — signing in or pointing at a
    /// new server must not need a restart.
    async fn relay_flux(self: Arc<Self>) {
        loop {
            let config = self.engine.settings().await.flux;
            let watcher = FluxWatcher::start(config.clone());
            let mut events = watcher.subscribe();
            let mut check = tokio::time::interval(Duration::from_secs(3));

            loop {
                tokio::select! {
                    event = events.recv() => match event {
                        Ok(event) => {
                            let _ = self.flux_events.send(event.clone());
                            if matches!(event, FluxEvent::Changed(_) | FluxEvent::Invalidated) {
                                self.engine.tick_auto().await;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(broadcast::error::RecvError::Closed) => break,
                    },
                    _ = check.tick() => {
                        if self.engine.settings().await.flux != config {
                            break;
                        }
                    }
                }
            }
        }
    }

    // --- Settings ----------------------------------------------------------

    pub async fn settings(&self) -> Settings {
        self.engine.settings().await
    }

    /// Persist new settings and hand them to the engine.
    ///
    /// Switching remote access on with no token mints one here, so the
    /// interface never has to invent secrets of its own.
    pub async fn save_settings(&self, mut settings: Settings) -> Response<()> {
        if settings.remote.enabled && settings.remote.token.as_deref().unwrap_or("").is_empty() {
            settings.remote.token = Some(crate::config::RemoteConfig::generate_token());
        }
        self.persist(settings).await
    }

    async fn persist(&self, settings: Settings) -> Response<()> {
        self.store
            .save(&settings)
            .map_err(|error| error.to_string())?;
        self.engine.set_settings(settings).await;
        Ok(())
    }

    /// Insert or update one project's binding, leaving the rest of the settings alone.
    pub async fn save_binding(&self, binding: ProjectBinding) -> Response<()> {
        let mut settings = self.engine.settings().await;
        settings.upsert_binding(binding);
        self.persist(settings).await
    }

    /// Replace the remote token, logging every paired device out.
    pub async fn rotate_remote_token(&self) -> Response<String> {
        let mut settings = self.engine.settings().await;
        let token = crate::config::RemoteConfig::generate_token();
        settings.remote.token = Some(token.clone());
        self.persist(settings).await?;
        Ok(token)
    }

    /// Forget the stored Flux session cookie.
    pub async fn sign_out(&self) -> Response<()> {
        let mut settings = self.engine.settings().await;
        settings.flux.cookie = None;
        self.persist(settings).await
    }

    /// Keep a session cookie a sign-in window produced.
    pub async fn keep_cookie(&self, cookie: String) -> Response<()> {
        let mut settings = self.engine.settings().await;
        settings.flux.cookie = Some(cookie);
        self.persist(settings).await
    }

    // --- Trackers ----------------------------------------------------------

    async fn flux(&self) -> Response<FluxClient> {
        self.engine
            .flux_client()
            .await
            .map_err(|error| error.to_string())
    }

    /// The client for one project's tracker. `source` comes from the
    /// interface (which knows where a project was listed from); a saved
    /// binding is the fallback, and Flux the default, so pre-existing callers
    /// keep working.
    async fn source_client(
        &self,
        project_id: &str,
        source: Option<SourceKind>,
    ) -> Response<Arc<dyn TaskSource>> {
        let settings = self.engine.settings().await;
        let kind = source
            .or_else(|| settings.binding(project_id).map(|b| b.source))
            .unwrap_or_default();
        Engine::source_for(&settings, kind).map_err(|error| error.to_string())
    }

    pub async fn test_connection(&self) -> Response<ConnectionState> {
        let settings = self.engine.settings().await;
        let warnings = settings.flux.access_warnings();

        let client = match self.flux().await {
            Ok(client) => client,
            Err(error) => {
                return Ok(ConnectionState {
                    connected: false,
                    error: Some(error),
                    kind: "unreachable",
                    warnings,
                })
            }
        };

        // Listing projects exercises the whole path: the proxy, then Flux's own key.
        match client.list_projects().await {
            Ok(_) => {
                // Success is not proof of authentication. A server that requires a
                // key still answers a keyless GET with the public projects — an
                // empty list for a private board, which looks exactly like a
                // connected server with nothing on it.
                if let Ok(status) = client.auth_status().await {
                    if status.needs_key() {
                        let base = settings.flux.normalised_base();
                        return Ok(ConnectionState {
                            connected: false,
                            error: Some(format!(
                                "This Flux server requires an API key, and none was accepted — so only \
public projects are visible. Create a key at {base}/auth and paste it above."
                            )),
                            kind: "flux_auth",
                            warnings,
                        });
                    }
                }

                Ok(ConnectionState {
                    connected: true,
                    error: None,
                    kind: "ok",
                    warnings,
                })
            }
            Err(error) => {
                let (kind, message) = if error.is_proxy_challenge() {
                    ("proxy_challenge", error.to_string())
                } else if error.is_auth() {
                    (
                        "flux_auth",
                        "Flux rejected the API key. Check it in Settings.".to_string(),
                    )
                } else {
                    ("unreachable", error.to_string())
                };

                Ok(ConnectionState {
                    connected: false,
                    error: Some(message),
                    kind,
                    warnings,
                })
            }
        }
    }

    /// Exercise the Linear connection: one authenticated whoami round trip.
    pub async fn test_linear_connection(&self) -> Response<ConnectionState> {
        let settings = self.engine.settings().await;

        if !settings.linear_enabled() {
            return Ok(ConnectionState {
                connected: false,
                error: Some("No Linear API key is set.".into()),
                kind: "unconfigured",
                warnings: Vec::new(),
            });
        }

        let client = match LinearClient::new(settings.linear.clone().unwrap_or_default()) {
            Ok(client) => client,
            Err(error) => {
                return Ok(ConnectionState {
                    connected: false,
                    error: Some(error.to_string()),
                    kind: "unreachable",
                    warnings: Vec::new(),
                })
            }
        };

        match client.viewer_name().await {
            Ok(_) => Ok(ConnectionState {
                connected: true,
                error: None,
                kind: "ok",
                warnings: Vec::new(),
            }),
            Err(error) => Ok(ConnectionState {
                connected: false,
                error: Some(error.to_string()),
                kind: if error.is_auth() {
                    "linear_auth"
                } else {
                    "unreachable"
                },
                warnings: Vec::new(),
            }),
        }
    }

    /// Projects from every configured tracker, each stamped with its source.
    ///
    /// One tracker failing must not blank the other's board, so failures are
    /// only fatal when nothing could be listed at all; the per-tracker
    /// connection tests in Settings are where a broken credential gets
    /// diagnosed.
    pub async fn list_projects(&self) -> Response<Vec<Project>> {
        let settings = self.engine.settings().await;

        let mut projects: Vec<Project> = Vec::new();
        let mut failures: Vec<String> = Vec::new();

        match self.flux().await {
            Ok(flux) => match flux.list_projects().await {
                Ok(mut listed) => projects.append(&mut listed),
                Err(error) => failures.push(error.to_string()),
            },
            Err(error) => failures.push(error),
        }

        if settings.linear_enabled() {
            match Engine::source_for(&settings, SourceKind::Linear) {
                Ok(linear) => match linear.list_projects().await {
                    Ok(mut listed) => projects.append(&mut listed),
                    Err(error) => failures.push(error.to_string()),
                },
                Err(error) => failures.push(error.to_string()),
            }
        }

        if projects.is_empty() {
            if let Some(failure) = failures.into_iter().next() {
                return Err(failure);
            }
        }
        Ok(projects)
    }

    pub async fn board(&self, project_id: &str, source: Option<SourceKind>) -> Response<BoardView> {
        let client = self.source_client(project_id, source).await?;

        let (project, epics, tasks) = tokio::try_join!(
            client.get_project(project_id),
            client.list_epics(project_id),
            client.list_tasks(project_id),
        )
        .map_err(|error| error.to_string())?;

        let running: HashSet<String> = self.engine.running_task_ids().await;
        let board = BoardSnapshot {
            epics: &epics,
            tasks: &tasks,
        };

        let ready: Vec<String> = board
            .candidates(&running)
            .into_iter()
            .map(|candidate| candidate.task.id)
            .collect();

        let views = tasks
            .iter()
            .map(|task| TaskView {
                task: task.clone(),
                ineligible: board.eligibility(task, &running).map(|why| why.describe()),
            })
            .collect();

        Ok(BoardView {
            project,
            epics,
            tasks: views,
            ready,
        })
    }

    /// Flip an epic's Auto switch.
    ///
    /// On Flux this writes to the server, so the change is visible on the board
    /// and to anything else watching it. Linear has no such field, so the flag is
    /// Heretic's own, kept in settings alongside the connection.
    pub async fn set_epic_auto(
        &self,
        epic_id: &str,
        auto: bool,
        source: Option<SourceKind>,
    ) -> Response<()> {
        match source.unwrap_or_default() {
            SourceKind::Flux => self
                .flux()
                .await?
                .set_epic_auto(epic_id, auto)
                .await
                .map(|_| ())
                .map_err(|error| error.to_string()),
            SourceKind::Linear => {
                let mut settings = self.engine.settings().await;
                let linear = settings.linear.get_or_insert_with(Default::default);
                if auto {
                    if !linear.auto_epics.iter().any(|id| id == epic_id) {
                        linear.auto_epics.push(epic_id.to_string());
                    }
                } else {
                    linear.auto_epics.retain(|id| id != epic_id);
                }
                self.persist(settings).await
            }
        }
    }

    // --- Runs --------------------------------------------------------------

    pub async fn runs(&self) -> Vec<RunRecord> {
        self.engine.runs().await
    }

    pub async fn start_task(&self, project_id: &str, task_id: &str) -> Response<String> {
        self.engine
            .start_task(project_id, task_id)
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn stop_run(&self, run_id: &str) -> bool {
        self.engine.stop_run(run_id).await
    }

    /// Answer the question a paused run is waiting on. Returns false when the
    /// run is no longer waiting — stopped, finished, or already answered.
    pub async fn answer_question(&self, run_id: &str, answer: String) -> bool {
        self.engine.answer_question(run_id, answer).await
    }

    pub async fn dismiss_run(&self, run_id: &str) -> bool {
        self.engine.dismiss_run(run_id).await
    }

    /// Merge a finished run's branch into the branch it came from, then
    /// remove its worktree.
    pub async fn integrate_run(&self, run_id: &str) -> Response<()> {
        self.engine
            .integrate_run(run_id)
            .await
            .map_err(|error| error.to_string())
    }

    /// Throw a finished run's work away: worktree removed, branch deleted.
    pub async fn discard_run_work(&self, run_id: &str) -> Response<()> {
        self.engine
            .discard_run_work(run_id)
            .await
            .map_err(|error| error.to_string())
    }

    /// Every file a run touched, with its line counts — the list behind the
    /// Changes tab.
    pub async fn run_changed_files(&self, run_id: &str) -> Response<Vec<FileChange>> {
        self.engine
            .run_changed_files(run_id)
            .await
            .map_err(|error| error.to_string())
    }

    /// One file's diff, as a unified patch.
    pub async fn run_file_diff(&self, run_id: &str, path: &str) -> Response<String> {
        self.engine
            .run_file_diff(run_id, path)
            .await
            .map_err(|error| error.to_string())
    }

    /// The commits a run put on its branch, newest first.
    pub async fn run_commits(&self, run_id: &str) -> Response<Vec<Commit>> {
        self.engine
            .run_commits(run_id)
            .await
            .map_err(|error| error.to_string())
    }

    /// The patch one of those commits introduced.
    pub async fn run_commit_diff(&self, run_id: &str, sha: &str) -> Response<String> {
        self.engine
            .run_commit_diff(run_id, sha)
            .await
            .map_err(|error| error.to_string())
    }

    /// Start whatever auto-enabled work is ready right now.
    pub async fn tick_auto(&self) -> Vec<String> {
        self.engine.tick_auto().await
    }

    /// The board in a glance.
    pub async fn summary(&self) -> Summary {
        summarise(&self.engine.runs().await)
    }

    // --- Discovering what is available to run ------------------------------

    /// Scan for agent CLIs and model hosts.
    ///
    /// Hosts are probed concurrently: a machine that is asleep should not hold
    /// up the ones that are awake.
    pub async fn detect_environment(&self) -> Environment {
        let settings = self.engine.settings().await;
        let (clis, hosts) =
            tokio::join!(detect::probe_clis(), detect::probe_hosts(&settings.hosts));

        Environment {
            clis,
            hosts,
            os: std::env::consts::OS,
        }
    }

    /// Look at one address without saving it, so a host can be checked before
    /// it is added.
    pub async fn probe_host(&self, name: String, base_url: String) -> HostProbe {
        let host = ModelHost {
            id: "probe".into(),
            name,
            base_url,
        };
        detect::probe_host(&host).await
    }

    /// Add or update a model host.
    pub async fn save_host(&self, host: ModelHost) -> Response<()> {
        let mut settings = self.engine.settings().await;
        // Store the address in a canonical form so `/v1` pasted by hand does not
        // become `/v1/v1` later.
        let host = ModelHost {
            base_url: detect::normalise_host_base(&host.base_url),
            ..host
        };
        settings.upsert_host(host);
        self.persist(settings).await
    }

    pub async fn remove_host(&self, host_id: &str) -> Response<()> {
        let mut settings = self.engine.settings().await;
        settings.remove_host(host_id);
        self.persist(settings).await
    }
}

/// Reduce a list of runs to the glance a ticker shows.
pub fn summarise(runs: &[RunRecord]) -> Summary {
    let active = runs.iter().filter(|run| run.is_active()).count();
    let waiting = runs
        .iter()
        .filter(|run| run.status == RunStatus::Waiting)
        .count();
    let attention = runs
        .iter()
        .filter(|run| run.status == RunStatus::NeedsAttention)
        .count();
    let unmerged = runs
        .iter()
        .filter(|run| !run.is_active() && run.landing == crate::orchestrator::Landing::OnBranch)
        .count();

    let mut parts = Vec::new();
    if active > 0 {
        parts.push(format!("{active} running"));
    }
    if waiting > 0 {
        parts.push(format!("{waiting} waiting for you"));
    }
    if attention > 0 {
        parts.push(format!(
            "{attention} need{} attention",
            if attention == 1 { "s" } else { "" }
        ));
    }
    if unmerged > 0 {
        parts.push(format!("{unmerged} to merge"));
    }
    let line = if parts.is_empty() {
        "Heretic is idle".to_string()
    } else {
        parts.join(" · ")
    };

    let glance = |run: &RunRecord| RunGlance {
        id: run.id.clone(),
        project_name: run.project_name.clone(),
        task_title: run.task_title.clone(),
        status: run.status,
        stage: run.stage,
        agent: run.agent.clone(),
        branch: run.branch.clone(),
        started_at: run.started_at.clone(),
        question: run.question.as_ref().map(|q| q.question.clone()),
    };

    let mut listed: Vec<RunGlance> = runs
        .iter()
        .filter(|run| run.is_active())
        .map(glance)
        .collect();
    listed.extend(
        runs.iter()
            .filter(|run| {
                !run.is_active()
                    && (run.status == RunStatus::NeedsAttention
                        || run.landing == crate::orchestrator::Landing::OnBranch)
            })
            .map(glance),
    );

    Summary {
        active,
        waiting,
        attention,
        unmerged,
        line,
        runs: listed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator::{Landing, PendingQuestion, RunResult};

    fn run(id: &str, status: RunStatus) -> RunRecord {
        RunRecord {
            id: id.into(),
            project_id: "proj".into(),
            project_name: "Project".into(),
            task_id: format!("task-{id}"),
            task_title: format!("Task {id}"),
            epic_title: String::new(),
            status,
            stage: RunStage::Implementing,
            agent: None,
            started_at: "2026-01-01T00:00:00Z".into(),
            finished_at: None,
            revisions: 0,
            branch: None,
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
    fn an_empty_board_is_idle() {
        let summary = summarise(&[]);
        assert_eq!(summary.line, "Heretic is idle");
        assert!(summary.runs.is_empty());
    }

    #[test]
    fn the_line_counts_what_needs_a_person() {
        let mut waiting = run("w", RunStatus::Waiting);
        waiting.question = Some(PendingQuestion {
            stage: RunStage::Implementing,
            role: None,
            question: "Which database?".into(),
        });
        let mut on_branch = run("b", RunStatus::Succeeded);
        on_branch.landing = Landing::OnBranch;
        on_branch.result = Some(RunResult::Completed);
        let mut merged = run("m", RunStatus::Succeeded);
        merged.landing = Landing::Merged;
        let attention = run("a", RunStatus::NeedsAttention);

        let summary = summarise(&[
            run("r", RunStatus::Running),
            waiting,
            on_branch,
            merged,
            attention,
        ]);

        assert_eq!(summary.active, 2);
        assert_eq!(summary.waiting, 1);
        assert_eq!(summary.attention, 1);
        assert_eq!(summary.unmerged, 1);
        assert_eq!(
            summary.line,
            "2 running · 1 waiting for you · 1 needs attention · 1 to merge"
        );

        // Active runs first, then the ones needing a decision; the merged run
        // is nobody's concern any more.
        let ids: Vec<&str> = summary.runs.iter().map(|run| run.id.as_str()).collect();
        assert_eq!(ids, ["r", "w", "b", "a"]);
        assert_eq!(summary.runs[1].question.as_deref(), Some("Which database?"));
    }
}
