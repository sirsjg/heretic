/**
 * Demo data and a simulated engine.
 *
 * Used when the UI runs in a plain browser (`pnpm dev`) rather than inside the
 * desktop shell, so the interface can be built and reviewed without agents, a
 * Flux server, or a repository. The shapes match `heretic-core` exactly.
 */

import type {
  BoardView,
  EngineEvent,
  Epic,
  Project,
  RunFeedItem,
  RunRecord,
  Settings,
  StageStats,
  Task,
  TaskView,
} from "./types";

const PROJECTS: Project[] = [
  {
    id: "mgdr6ar",
    name: "Corporate Travel",
    description:
      "Self-service corporate travel management platform for SMEs. API-first, with a canonical travel data model every supplier normalises into.",
    visibility: "private",
  },
  {
    id: "kb31xz9",
    name: "Flux",
    description: "The board itself — an execution-agnostic task engine.",
    visibility: "public",
  },
];

const EPICS: Epic[] = [
  {
    id: "m96cuus",
    title: "E01 · Platform Foundation & Developer Experience",
    status: "in_progress",
    notes:
      "Monorepo, Next.js App Router + TypeScript, Prisma/Postgres, CI/CD, observability, testing harness. Everything else depends on this.",
    auto: true,
    depends_on: [],
    project_id: "mgdr6ar",
  },
  {
    id: "iwmojt6",
    title: "E02 · API-First Core & Canonical Travel Data Model",
    status: "planning",
    notes:
      "The public API is the single boundary consumed by the web UI, the mobile app and the LLM layer.",
    auto: true,
    depends_on: ["m96cuus"],
    project_id: "mgdr6ar",
  },
  {
    id: "p3ap214",
    title: "E03 · Identity, Auth, SSO & Role-Based Access",
    status: "planning",
    notes: "Four roles: Traveller, Arranger, Approver, Admin. Tenant isolation at the data layer.",
    auto: false,
    depends_on: [],
    project_id: "mgdr6ar",
  },
  {
    id: "z7efw3p",
    title: "E22 · Design & Visual System (deferred — do last)",
    status: "planning",
    notes: "Intentionally last. Until this epic, all UI uses neutral defaults.",
    auto: false,
    depends_on: [],
    project_id: "mgdr6ar",
  },
];

function task(partial: Partial<Task> & Pick<Task, "id" | "title">): Task {
  return {
    status: "todo",
    depends_on: [],
    project_id: "mgdr6ar",
    epic_id: "m96cuus",
    blocked: false,
    archived: false,
    priority: 1,
    acceptance_criteria: [],
    guardrails: [],
    comments: [],
    workers: [],
    created_at: "2026-08-21T03:31:49.134Z",
    ...partial,
  };
}

const TASKS: Task[] = [
  task({
    id: "qkwq5z1",
    title:
      "TASK 0 · Establish /docs knowledge base and agent entry point before any code",
    priority: 0,
    acceptance_criteria: [
      "Repo root contains AGENTS.md that every coding agent reads first",
      "docs/README.md indexes the knowledge base with a read-this-before-X map",
      "docs/adr/ uses the MADR template with numbered ADRs",
    ],
    guardrails: [
      {
        id: "6hg0mty",
        number: 9999,
        text: "This task must be merged before any other task is started.",
      },
      {
        id: "boyuogj",
        number: 999,
        text: "Every ADR must explain WHY, not only WHAT.",
      },
    ],
    comments: [
      {
        id: "c1",
        body: "Agreed to use MADR over the lighter Nygard format — the Consequences section is the point.",
        author: "mcp",
        agent_name: "Claude Code · Orchestrator",
        created_at: "2026-08-21T09:12:00.000Z",
      },
    ],
  }),
  task({
    id: "f0myadx",
    title: "CI/CD pipeline: lint, typecheck, unit + integration tests, preview deploys",
    priority: 0,
    acceptance_criteria: [
      "GitHub Actions runs lint, typecheck and unit tests on every PR in under 10 minutes",
      "Every PR gets a preview deployment with its own ephemeral database",
    ],
    guardrails: [
      {
        id: "ljjuv2t",
        number: 100,
        text: "Hosting region for staging and production must be Australia.",
      },
    ],
  }),
  task({
    id: "t5cw8zx",
    title: "Observability baseline: structured logging, tracing, error tracking",
    priority: 1,
    acceptance_criteria: [
      "Structured JSON logger with tenant_id, user_id and request_id on every line",
      "PII is redacted by the logger before emission",
    ],
    guardrails: [
      {
        id: "hm9aohc",
        number: 500,
        text: "Never log full supplier request bodies at info level in production.",
      },
    ],
  }),
  task({
    id: "dpw789j",
    title: "Scaffold monorepo with Next.js (App Router), TypeScript strict, pnpm workspaces",
    priority: 0,
    depends_on: ["qkwq5z1"],
    blocked: true,
  }),
  task({
    id: "koe6pke",
    title: "Set up Postgres + Prisma with multi-tenant base schema",
    priority: 0,
    depends_on: ["qkwq5z1"],
    blocked: true,
  }),
  task({
    id: "m7iwj7m",
    title: "Background jobs & scheduling infrastructure (queue, retries, cron)",
    priority: 1,
    status: "planning",
  }),
  task({
    id: "t7ply95",
    title: "Testing harness: Vitest unit, integration with test DB, Playwright e2e",
    priority: 1,
    blocked_reason: "Heretic: review still requested changes after 3 attempts",
  }),
  task({
    id: "5cizcoz",
    title: "Feature flags, environment config and tenant-level settings service",
    priority: 2,
  }),
  task({
    id: "api001",
    title: "Define the canonical Offer and Booking schema",
    epic_id: "iwmojt6",
    priority: 1,
  }),
  task({
    id: "auth001",
    title: "Magic-link sign-in for the POC",
    epic_id: "p3ap214",
    priority: 1,
  }),
];

const DEFAULT_SETTINGS: Settings = {
  flux: {
    base_url: "https://flux.example.com",
    api_key: "flx_demo_key",
    headers: { "CF-Access-Client-Id": "demo.access" },
    cookie: null,
  },
  profiles: [
    {
      id: "claude-code",
      name: "Claude Code",
      runner: { kind: "claude_code" },
      model: null,
      extra_args: [],
      env: {},
      timeout_secs: 3600,
      autonomous: true,
    },
    {
      id: "qwen-local",
      name: "Qwen3 Coder (local)",
      runner: { kind: "codex_oss", base_url: "http://localhost:11434/v1" },
      model: "qwen3-coder:30b",
      extra_args: [],
      env: {},
      timeout_secs: 5400,
      autonomous: true,
    },
    {
      id: "codex-cloud",
      name: "Codex",
      runner: { kind: "codex" },
      model: "gpt-5-codex",
      extra_args: [],
      env: {},
      timeout_secs: 3600,
      autonomous: true,
    },
  ],
  roles: {
    orchestrator: "claude-code",
    implementer: "qwen-local",
    reviewer: "claude-code",
    documenter: "qwen-local",
  },
  hosts: [
    { id: "local-ollama", name: "This machine", base_url: "http://localhost:11434" },
    { id: "spark", name: "DGX Spark", base_url: "http://spark.local:11434" },
  ],
  bindings: [
    {
      project_id: "mgdr6ar",
      repo_path: "/Users/steve/code/corporate-travel",
      base_branch: "main",
      isolation: "worktree",
      integration: "leave",
      pipeline: { plan: true, review: true, document: false, max_revisions: 2 },
      roles: {},
      auto_run: true,
      max_parallel: 2,
    },
  ],
};

/** Why a task cannot be started, mirroring the Rust selection rules. */
function ineligibility(t: Task, epics: Epic[]): string | null {
  if (t.archived) return "Archived";
  const epic = epics.find((e) => e.id === t.epic_id);
  if (!epic) return "Not in an epic";
  if (!epic.auto) return "Auto is off for this epic";
  if (epic.depends_on.some((d) => epics.find((e) => e.id === d)?.status !== "done"))
    return "Epic is waiting on another epic";
  if (t.status !== "todo") return "Not in Todo";
  if (t.blocked_reason) return "Blocked externally";
  if (t.blocked) return "Blocked by a dependency";
  return null;
}

const RUN_SCRIPT: RunFeedItem[] = [
  { stage: "planning", role: "orchestrator", event: { type: "text", text: "Reading the repository to work out where documentation should live." } },
  { stage: "planning", role: "orchestrator", event: { type: "tool", name: "Glob", detail: "docs/**/*.md" } },
  { stage: "planning", role: "orchestrator", event: { type: "text", text: "Brief: create docs/README.md as an index, seed 15 ADRs using MADR, and add a CI check that fails when core packages change without docs." } },
  { stage: "implementing", role: "implementer", event: { type: "text", text: "Starting on the knowledge base." } },
  { stage: "implementing", role: "implementer", event: { type: "tool", name: "Write", detail: "AGENTS.md" } },
  { stage: "implementing", role: "implementer", event: { type: "tool", name: "Write", detail: "docs/README.md" } },
  { stage: "implementing", role: "implementer", event: { type: "tool", name: "Write", detail: "docs/adr/0001-monorepo-nextjs.md" } },
  { stage: "implementing", role: "implementer", event: { type: "tool", name: "Bash", detail: "pnpm lint" } },
  { stage: "implementing", role: "implementer", event: { type: "result", text: "Added AGENTS.md, docs/README.md and 15 ADRs. Lint passes.", is_error: false, duration_ms: 184000, cost_usd: null } },
  { stage: "reviewing", role: "reviewer", event: { type: "text", text: "Checking the ADRs against the acceptance criteria." } },
  { stage: "reviewing", role: "reviewer", event: { type: "tool", name: "Read", detail: "docs/adr/0008-llm-gateway.md" } },
  { stage: "reviewing", role: "reviewer", event: { type: "text", text: "ADR 0008 has no Consequences section, which guardrail 999 requires." } },
  { stage: "reviewing", role: "reviewer", event: { type: "result", text: "Two ADRs are missing Consequences.\n\nVERDICT: request_changes", is_error: false, duration_ms: 41000, cost_usd: 0.09 } },
  { stage: "implementing", role: "implementer", event: { type: "text", text: "Adding the missing Consequences sections." } },
  { stage: "implementing", role: "implementer", event: { type: "tool", name: "Edit", detail: "docs/adr/0008-llm-gateway.md" } },
  { stage: "implementing", role: "implementer", event: { type: "result", text: "Both ADRs now document their consequences.", is_error: false, duration_ms: 52000, cost_usd: null } },
  { stage: "reviewing", role: "reviewer", event: { type: "result", text: "All criteria satisfied.\n\nVERDICT: approve", is_error: false, duration_ms: 22000, cost_usd: 0.04 } },
];

/** The stats the engine would have collected for the scripted run above. */
const RUN_STATS: StageStats[] = [
  {
    stage: "planning",
    role: "orchestrator",
    agent: "Claude Code · Orchestrator",
    duration_ms: 63000,
    usage: { input_tokens: 2100, output_tokens: 1450, cache_read_tokens: 182000, cache_creation_tokens: 8400 },
    cost_usd: 0.11,
    models: [
      {
        model: "claude-sonnet-5",
        usage: { input_tokens: 2100, output_tokens: 1450, cache_read_tokens: 182000, cache_creation_tokens: 8400 },
        cost_usd: 0.11,
      },
    ],
  },
  {
    stage: "implementing",
    role: "implementer",
    agent: "Qwen3 Coder (local) · Implementer",
    duration_ms: 184000,
    usage: { input_tokens: 46000, output_tokens: 9800, cache_read_tokens: 152000, cache_creation_tokens: 0 },
    cost_usd: null,
    models: [
      {
        model: "qwen3-coder:30b",
        usage: { input_tokens: 46000, output_tokens: 9800, cache_read_tokens: 152000, cache_creation_tokens: 0 },
        cost_usd: null,
      },
    ],
  },
  {
    stage: "reviewing",
    role: "reviewer",
    agent: "Claude Code · Reviewer",
    duration_ms: 41000,
    usage: { input_tokens: 900, output_tokens: 620, cache_read_tokens: 96000, cache_creation_tokens: 3800 },
    cost_usd: 0.09,
    models: [
      {
        model: "claude-sonnet-5",
        usage: { input_tokens: 900, output_tokens: 620, cache_read_tokens: 96000, cache_creation_tokens: 3800 },
        cost_usd: 0.09,
      },
    ],
  },
  {
    stage: "implementing",
    role: "implementer",
    agent: "Qwen3 Coder (local) · Implementer",
    duration_ms: 52000,
    usage: { input_tokens: 5200, output_tokens: 2900, cache_read_tokens: 62000, cache_creation_tokens: 0 },
    cost_usd: null,
    models: [
      {
        model: "qwen3-coder:30b",
        usage: { input_tokens: 5200, output_tokens: 2900, cache_read_tokens: 62000, cache_creation_tokens: 0 },
        cost_usd: null,
      },
    ],
  },
  {
    stage: "reviewing",
    role: "reviewer",
    agent: "Claude Code · Reviewer",
    duration_ms: 22000,
    usage: { input_tokens: 300, output_tokens: 210, cache_read_tokens: 98000, cache_creation_tokens: 900 },
    cost_usd: 0.04,
    models: [
      {
        model: "claude-sonnet-5",
        usage: { input_tokens: 300, output_tokens: 210, cache_read_tokens: 98000, cache_creation_tokens: 900 },
        cost_usd: 0.04,
      },
    ],
  },
];

let runCounter = 0;

/** A simulated engine that streams a scripted run, for developing the UI. */
export class MockEngine {
  private runs = new Map<string, RunRecord>();
  private listeners = new Set<(event: EngineEvent) => void>();
  private settings: Settings = structuredClone(DEFAULT_SETTINGS);
  private epics = structuredClone(EPICS);
  private timers = new Set<ReturnType<typeof setTimeout>>();

  listProjects(): Project[] {
    return PROJECTS;
  }

  getSettings(): Settings {
    return structuredClone(this.settings);
  }

  saveSettings(settings: Settings) {
    this.settings = structuredClone(settings);
  }

  board(projectId: string): BoardView {
    const project = PROJECTS.find((p) => p.id === projectId) ?? PROJECTS[0]!;
    const epics = this.epics.filter((e) => e.project_id === projectId);
    const tasks = TASKS.filter((t) => t.project_id === projectId);
    const views: TaskView[] = tasks.map((t) => ({
      task: t,
      ineligible: ineligibility(t, epics),
    }));
    return {
      project,
      epics,
      tasks: views,
      ready: views
        .filter((v) => v.ineligible === null)
        .sort((a, b) => (a.task.priority ?? 1) - (b.task.priority ?? 1))
        .map((v) => v.task.id),
    };
  }

  setEpicAuto(epicId: string, auto: boolean) {
    const epic = this.epics.find((e) => e.id === epicId);
    if (epic) epic.auto = auto;
  }

  listRuns(): RunRecord[] {
    return [...this.runs.values()].sort((a, b) =>
      b.started_at.localeCompare(a.started_at),
    );
  }

  subscribe(listener: (event: EngineEvent) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private emit(event: EngineEvent) {
    for (const listener of this.listeners) listener(event);
  }

  private later(fn: () => void, ms: number) {
    const timer = setTimeout(() => {
      this.timers.delete(timer);
      fn();
    }, ms);
    this.timers.add(timer);
  }

  startTask(projectId: string, taskId: string): string {
    const board = this.board(projectId);
    const task = board.tasks.find((t) => t.task.id === taskId)?.task;
    const id = `run-${++runCounter}`;

    const run: RunRecord = {
      id,
      project_id: projectId,
      project_name: board.project.name,
      task_id: taskId,
      task_title: task?.title ?? taskId,
      epic_title:
        board.epics.find((e) => e.id === task?.epic_id)?.title ?? "",
      status: "running",
      stage: "preparing",
      agent: "Qwen3 Coder (local) · Implementer",
      started_at: new Date().toISOString(),
      revisions: 0,
      branch: `heretic/${taskId}`,
      base_branch: "main",
      landing: "nothing",
      worktree_path: `~/.local/share/heretic/worktrees/${projectId}/${taskId}`,
      changes: { files_changed: 0, insertions: 0, deletions: 0, files: [] },
      stats: [],
      feed: [],
    };

    this.runs.set(id, run);
    this.emit({ kind: "run_updated", run: structuredClone(run) });
    this.play(id, 0);
    return id;
  }

  /** Replay the scripted run, one beat at a time. */
  private play(runId: string, index: number) {
    const run = this.runs.get(runId);
    if (!run || run.status !== "running") return;

    if (index >= RUN_SCRIPT.length) {
      run.status = "succeeded";
      run.stage = "integrating";
      run.result = { kind: "completed" };
      run.finished_at = new Date().toISOString();
      run.landing = "on_branch";
      run.stats = structuredClone(RUN_STATS);
      run.changes = {
        files_changed: 18,
        insertions: 1240,
        deletions: 12,
        files: ["AGENTS.md", "docs/README.md", "docs/adr/0001-monorepo-nextjs.md"],
      };
      this.emit({ kind: "run_updated", run: structuredClone(run) });
      return;
    }

    const item = RUN_SCRIPT[index]!;
    if (item.stage !== run.stage) {
      run.stage = item.stage;
      if (item.role) {
        const profileId = this.settings.roles[item.role];
        const profile = this.settings.profiles.find((p) => p.id === profileId);
        run.agent = profile
          ? `${profile.name} · ${item.role[0]!.toUpperCase()}${item.role.slice(1)}`
          : run.agent;
      }
      this.emit({ kind: "run_updated", run: structuredClone(run) });
    }

    // A rejected review sends the work back — reflect that in the counter.
    if (
      item.event.type === "result" &&
      item.event.text?.includes("request_changes")
    ) {
      run.revisions += 1;
      this.emit({ kind: "run_updated", run: structuredClone(run) });
    }

    run.feed.push(item);
    this.emit({ kind: "run_output", run_id: runId, item });
    this.later(() => this.play(runId, index + 1), 620);
  }

  stopRun(runId: string): boolean {
    const run = this.runs.get(runId);
    if (!run || run.status !== "running") return false;
    run.status = "cancelled";
    run.result = { kind: "cancelled" };
    run.finished_at = new Date().toISOString();
    this.emit({ kind: "run_updated", run: structuredClone(run) });
    return true;
  }

  dismissRun(runId: string): boolean {
    const run = this.runs.get(runId);
    if (!run || run.status === "running") return false;
    this.runs.delete(runId);
    return true;
  }
}
