/**
 * Mirrors the serialised types from `accel-core`.
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

/** A task plus why Accelerate can or cannot start it. */
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
}

export interface Settings {
  flux: FluxConfig;
  profiles: ModelProfile[];
  roles: Partial<Record<Role, string>>;
  bindings: ProjectBinding[];
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
  worktree_path?: string | null;
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
}

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
