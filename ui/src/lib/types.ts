/**
 * Mirrors the serialised types from `heretic-core`.
 *
 * Anything the Rust side sends over a Tauri command or event has a shape here.
 */

export type Priority = 0 | 1 | 2;
export type TaskStatus = "planning" | "todo" | "in_progress" | "done";
export type Role = "orchestrator" | "implementer" | "reviewer" | "documenter";

export const ROLES: Role[] = [
  "orchestrator",
  "implementer",
  "reviewer",
  "documenter",
];

export const ROLE_LABELS: Record<Role, string> = {
  orchestrator: "Orchestrator",
  implementer: "Implementer",
  reviewer: "Reviewer",
  documenter: "Documenter",
};

export const ROLE_BLURBS: Record<Role, string> = {
  orchestrator: "Reads the task and writes the brief the implementer works from.",
  implementer: "Does the work in the repository.",
  reviewer: "Reads the diff and decides whether it ships.",
  documenter: "Updates the docs once the work is approved.",
};

export interface Project {
  id: string;
  name: string;
  description?: string | null;
  visibility?: string | null;
}

export interface Epic {
  id: string;
  title: string;
  status: string;
  notes: string;
  auto: boolean;
  depends_on: string[];
  project_id: string;
}

export interface Guardrail {
  id: string;
  number: number;
  text: string;
}

export interface TaskComment {
  id: string;
  body: string;
  author?: string | null;
  agent_name?: string | null;
  created_at?: string | null;
}

export interface Task {
  id: string;
  title: string;
  status: string;
  notes?: string | null;
  depends_on: string[];
  project_id: string;
  epic_id?: string | null;
  blocked: boolean;
  blocked_reason?: string | null;
  archived: boolean;
  priority?: Priority | null;
  acceptance_criteria: string[];
  guardrails: Guardrail[];
  comments: TaskComment[];
  workers: string[];
  created_at?: string | null;
  updated_at?: string | null;
}

/** A task plus why Heretic can or cannot start it. */
export interface TaskView {
  task: Task;
  /** null when the task is ready to run. */
  ineligible: string | null;
}

export interface BoardView {
  project: Project;
  epics: Epic[];
  tasks: TaskView[];
  /** Task ids ready to start, most important first. */
  ready: string[];
}

// --- Configuration ----------------------------------------------------------

export type RunnerKind =
  | { kind: "claude_code" }
  | { kind: "codex" }
  | { kind: "codex_oss"; base_url?: string | null }
  | { kind: "custom"; command: string; args: string[] };

export interface ModelProfile {
  id: string;
  name: string;
  runner: RunnerKind;
  model?: string | null;
  extra_args: string[];
  env: Record<string, string>;
  timeout_secs?: number | null;
  /** Context window in tokens, when the host reports one. */
  context_window?: number | null;
  autonomous: boolean;
}

export type Isolation = "worktree" | "in_place";
export type IntegrationMode = "leave" | "merge";

export interface Pipeline {
  plan: boolean;
  review: boolean;
  document: boolean;
  max_revisions: number;
}

export interface ProjectBinding {
  project_id: string;
  repo_path: string;
  base_branch?: string | null;
  isolation: Isolation;
  integration: IntegrationMode;
  pipeline: Pipeline;
  roles: Partial<Record<Role, string>>;
  auto_run: boolean;
  max_parallel: number;
}

export interface FluxConfig {
  base_url: string;
  api_key?: string | null;
  /** Extra headers for the identity proxy in front of Flux. */
  headers: Record<string, string>;
  /** Session cookie from signing in, as it would appear in a Cookie header. */
  cookie?: string | null;
}

export interface Settings {
  flux: FluxConfig;
  profiles: ModelProfile[];
  roles: Partial<Record<Role, string>>;
  bindings: ProjectBinding[];
  hosts: ModelHost[];
}

// --- Discovery --------------------------------------------------------------

/** An agent CLI Heretic knows how to drive. */
export interface CliStatus {
  program: string;
  label: string;
  found: boolean;
  version?: string | null;
  problem?: string | null;
}

/** A machine serving models — this laptop, or a box on the network. */
export interface ModelHost {
  id: string;
  name: string;
  base_url: string;
}

export interface DiscoveredModel {
  id: string;
  parameter_size?: string | null;
  quantization?: string | null;
  size_bytes?: number | null;
  /** Context window in tokens, when the server reports one. */
  context_length?: number | null;
}

export interface HostProbe {
  host: ModelHost;
  reachable: boolean;
  kind?: "ollama" | "openai_compatible" | null;
  models: DiscoveredModel[];
  problem?: string | null;
  /** Server version, when it reports one. */
  version?: string | null;
  /** Something that will stop a run even though the host answered. */
  warning?: string | null;
}

export interface Environment {
  clis: CliStatus[];
  hosts: HostProbe[];
  os: string;
}

/** A one-line description of a model, matching the Rust side. */
export function describeModel(model: DiscoveredModel): string {
  const parts: string[] = [];
  if (model.parameter_size) parts.push(model.parameter_size);
  if (model.quantization) parts.push(model.quantization);
  if (typeof model.context_length === "number") {
    parts.push(`${Math.round(model.context_length / 1024)}k ctx`);
  }
  if (typeof model.size_bytes === "number") {
    parts.push(
      model.size_bytes >= 1e9
        ? `${(model.size_bytes / 1e9).toFixed(1)} GB`
        : `${Math.round(model.size_bytes / 1e6)} MB`,
    );
  }
  return parts.join(" · ");
}

// --- Runs -------------------------------------------------------------------

export type RunStage =
  | "preparing"
  | "planning"
  | "implementing"
  | "reviewing"
  | "documenting"
  | "integrating";

export type RunStatus =
  | "queued"
  | "running"
  | "succeeded"
  | "failed"
  | "cancelled"
  | "needs_attention";

export type AgentEvent =
  | { type: "text"; text: string }
  | { type: "tool"; name: string; detail?: string | null }
  | { type: "raw"; text: string }
  | { type: "error"; message: string }
  | {
      type: "result";
      text?: string | null;
      is_error: boolean;
      duration_ms?: number | null;
      cost_usd?: number | null;
    };

export interface RunFeedItem {
  stage: RunStage;
  role?: Role | null;
  event: AgentEvent;
}

/** Where a run's work ended up. */
export type Landing = "nothing" | "on_branch" | "merged" | "discarded";

export type RunResult =
  | { kind: "completed" }
  | { kind: "failed"; stage: RunStage; reason: string }
  | { kind: "cancelled" }
  | { kind: "needs_attention"; reason: string };

export interface ChangeSummary {
  files_changed: number;
  insertions: number;
  deletions: number;
  files: string[];
}

export interface RunRecord {
  id: string;
  project_id: string;
  project_name: string;
  task_id: string;
  task_title: string;
  epic_title: string;
  status: RunStatus;
  stage: RunStage;
  agent?: string | null;
  started_at: string;
  finished_at?: string | null;
  revisions: number;
  branch?: string | null;
  /** The branch the work forked from, and would merge back into. */
  base_branch?: string | null;
  worktree_path?: string | null;
  /** What has become of the work since the agents finished. */
  landing: Landing;
  changes: ChangeSummary;
  result?: RunResult | null;
  feed: RunFeedItem[];
}

export type EngineEvent =
  | { kind: "run_updated"; run: RunRecord }
  | { kind: "run_output"; run_id: string; item: RunFeedItem }
  | { kind: "notice"; level: string; message: string };

export interface ConnectionState {
  connected: boolean;
  /** Populated when the server rejected us or could not be reached. */
  error?: string | null;
  /** Which layer failed, so the interface can point at the right fix. */
  kind?: "ok" | "flux_auth" | "proxy_challenge" | "unreachable";
  /** Configuration that will bite later, even when the connection works. */
  warnings?: string[];
}

/** Header names Cloudflare Access expects for a service token. */
export const CF_ACCESS_ID = "CF-Access-Client-Id";
export const CF_ACCESS_SECRET = "CF-Access-Client-Secret";

export const STAGE_ORDER: RunStage[] = [
  "preparing",
  "planning",
  "implementing",
  "reviewing",
  "documenting",
  "integrating",
];

export const STAGE_LABELS: Record<RunStage, string> = {
  preparing: "Prepare",
  planning: "Plan",
  implementing: "Implement",
  reviewing: "Review",
  documenting: "Document",
  integrating: "Integrate",
};

export const PRIORITY_LABELS: Record<Priority, string> = {
  0: "P0",
  1: "P1",
  2: "P2",
};
