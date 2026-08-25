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
  FileChange,
  Project,
  RunCommit,
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
      repo_path: "/home/dev/code/corporate-travel",
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
  { stage: "planning", role: "orchestrator", event: { type: "prompt", text: "You are the orchestrator on an autonomous engineering team. Another agent will implement this task from your brief.\n\n## The task\n\n- Project: Corporate Travel\n- Task T-104: Build the knowledge base\n\n### Acceptance criteria\n\n- [ ] Every ADR follows MADR\n- [ ] CI fails when core packages change without docs" } },
  { stage: "planning", role: "orchestrator", event: { type: "text", text: "Reading the repository to work out where documentation should live." } },
  { stage: "planning", role: "orchestrator", event: { type: "tool", name: "Glob", detail: "docs/**/*.md" } },
  { stage: "planning", role: "orchestrator", event: { type: "text", text: "Brief: create docs/README.md as an index, seed 15 ADRs using MADR, and add a CI check that fails when core packages change without docs." } },
  { stage: "implementing", role: "implementer", event: { type: "prompt", text: "You are implementing one task end to end in this repository.\n\n## Implementation brief\n\nThe orchestrator prepared this plan. Follow it unless you find it to be wrong.\n\nCreate docs/README.md as an index, seed 15 ADRs using MADR, and add a CI check that fails when core packages change without docs." } },
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
  { stage: "implementing", role: "implementer", event: { type: "prompt", text: "You are implementing one task end to end in this repository.\n\n## Changes requested by review\n\nYour previous attempt was reviewed and sent back. Address every point below; do not start over.\n\nTwo ADRs are missing Consequences: docs/adr/0008-llm-gateway.md and docs/adr/0012-queueing.md." } },
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

/**
 * The work the scripted run left behind.
 *
 * Real diffs come from git; these exist so the Changes and History panels can be
 * designed and reviewed in a browser, with the same shapes the engine sends.
 */
const RUN_FILES: FileChange[] = [
  { path: "AGENTS.md", status: "added", insertions: 64, deletions: 0, binary: false },
  { path: ".github/workflows/docs.yml", status: "added", insertions: 31, deletions: 0, binary: false },
  { path: "docs/README.md", status: "added", insertions: 82, deletions: 0, binary: false },
  { path: "docs/adr/0001-monorepo-nextjs.md", status: "added", insertions: 46, deletions: 0, binary: false },
  { path: "docs/adr/0008-llm-gateway.md", status: "modified", insertions: 18, deletions: 3, binary: false },
  { path: "docs/adr/0012-queueing.md", status: "modified", insertions: 15, deletions: 2, binary: false },
  { path: "docs/architecture.md", old_path: "ARCHITECTURE.md", status: "renamed", insertions: 4, deletions: 4, binary: false },
  { path: "docs/assets/pipeline.png", status: "added", insertions: 0, deletions: 0, binary: true },
  { path: "scratch.md", status: "untracked", insertions: 3, deletions: 0, binary: false },
];

const DIFFS: Record<string, string> = {
  "docs/adr/0008-llm-gateway.md": `diff --git a/docs/adr/0008-llm-gateway.md b/docs/adr/0008-llm-gateway.md
index 3f7a91c..b2c4e08 100644
--- a/docs/adr/0008-llm-gateway.md
+++ b/docs/adr/0008-llm-gateway.md
@@ -18,9 +18,12 @@ ## Decision
 
 Route every model call through a single gateway service.
 
-Providers are selected per request.
+Providers are selected per request, and the gateway is the only component
+holding provider credentials.
 
 ## Consequences
 
-TODO
+- One place to add a provider, and one place to rate-limit.
+- The gateway is on the critical path for every feature that calls a model,
+  so it needs the same availability budget as the API itself.
+- Per-provider latency is visible in one dashboard rather than five.
`,
  "docs/architecture.md": `diff --git a/ARCHITECTURE.md b/docs/architecture.md
similarity index 94%
rename from ARCHITECTURE.md
rename to docs/architecture.md
--- a/ARCHITECTURE.md
+++ b/docs/architecture.md
@@ -1,7 +1,7 @@
-# Architecture
+# Architecture
 
-See the ADRs in ./adr for the decisions behind this.
+See the ADRs in ./adr for the decisions behind this, indexed in ./README.md.
 
 ## Services
`,
  "docs/assets/pipeline.png": `diff --git a/docs/assets/pipeline.png b/docs/assets/pipeline.png
new file mode 100644
Binary files /dev/null and b/docs/assets/pipeline.png differ
`,
  "scratch.md": `diff --git a/scratch.md b/scratch.md
new file mode 100644
--- /dev/null
+++ b/scratch.md
@@ -0,0 +1,3 @@
+# Scratch
+
+Notes to fold into the ADR index later.
`,
};

/** A plausible new-file patch for anything without a hand-written one above. */
function sampleDiff(file: FileChange): string {
  const lines = Math.min(file.insertions, 8);
  const body = Array.from(
    { length: lines },
    (_, index) => `+Line ${index + 1} of ${file.path}`,
  ).join("\n");

  return `diff --git a/${file.path} b/${file.path}
new file mode 100644
--- /dev/null
+++ b/${file.path}
@@ -0,0 +1,${lines} @@
${body}
`;
}

const RUN_COMMITS: RunCommit[] = [
  {
    sha: "9c1f4ab7d3e05a6b8c2d1e0f9a8b7c6d5e4f3a2b",
    short_sha: "9c1f4ab",
    author: "Heretic",
    email: "heretic@localhost",
    authored_at: "2026-08-24T14:41:00Z",
    subject: "Document the consequences of ADR 0008 and 0012",
    body: "Review sent the work back: both ADRs stated a decision with no consequences.",
    files_changed: 2,
    insertions: 33,
    deletions: 5,
  },
  {
    sha: "4d2e8bb0a1c93f5e7d6c4b3a2918e7f6d5c4b3a2",
    short_sha: "4d2e8bb",
    author: "Heretic",
    email: "heretic@localhost",
    authored_at: "2026-08-24T14:22:00Z",
    subject: "Seed the knowledge base with 15 ADRs",
    body: "",
    files_changed: 17,
    insertions: 1207,
    deletions: 7,
  },
];

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
      // Derived from the same fixture the Changes panel reads, so the header
      // summary and the file list cannot disagree the way they would if both
      // were written out by hand.
      run.changes = {
        files_changed: RUN_FILES.length,
        insertions: RUN_FILES.reduce((sum, file) => sum + file.insertions, 0),
        deletions: RUN_FILES.reduce((sum, file) => sum + file.deletions, 0),
        files: RUN_FILES.map((file) => file.path),
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

  // --- What the run left in the repository ---------------------------------

  runChangedFiles(runId: string): FileChange[] {
    // Nothing is on disk until the implementer has been through once.
    const run = this.runs.get(runId);
    if (!run) return [];
    const reached = run.feed.some((item) => item.stage !== "planning");
    return reached ? structuredClone(RUN_FILES) : [];
  }

  runFileDiff(_runId: string, path: string): string {
    const written = DIFFS[path];
    if (written) return written;
    const file = RUN_FILES.find((candidate) => candidate.path === path);
    return file ? sampleDiff(file) : "";
  }

  runCommits(runId: string): RunCommit[] {
    // A run commits when it finishes, so a live one has no history yet.
    const run = this.runs.get(runId);
    if (!run || run.status === "running" || run.status === "queued") return [];
    return structuredClone(RUN_COMMITS);
  }

  runCommitDiff(_runId: string, sha: string): string {
    const first = RUN_COMMITS[0];
    if (first && sha === first.sha) {
      return [DIFFS["docs/adr/0008-llm-gateway.md"] ?? ""].join("\n");
    }
    return [
      DIFFS["docs/architecture.md"] ?? "",
      sampleDiff(RUN_FILES[0]!),
      sampleDiff(RUN_FILES[2]!),
    ].join("\n");
  }
}
