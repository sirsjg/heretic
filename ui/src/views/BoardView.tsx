import { useMemo, useState } from "react";
import { useStore, currentBinding } from "../lib/store";
import type { Epic, Priority, RunRecord, TaskView } from "../lib/types";
import { PRIORITY_LABELS } from "../lib/types";
import { api } from "../lib/bridge";
import { count } from "../lib/format";
import { Badge, Button, Dot, EmptyState, Panel, Spinner, Toggle, cx } from "../components/ui";
import {
  IconAlert,
  IconBranch,
  IconChevron,
  IconFolder,
  IconPlay,
  IconRefresh,
  IconSparks,
} from "../components/icons";

const PRIORITY_TONE: Record<Priority, "danger" | "warn" | "neutral"> = {
  0: "danger",
  1: "warn",
  2: "neutral",
};

export function BoardView() {
  const {
    board,
    boardLoading,
    settings,
    refreshBoard,
    setEpicAuto,
    startTask,
    runReady,
    saveBinding,
    runs,
  } = useStore();
  const binding = useStore(currentBinding);
  const openRun = useStore((state) => state.openRun);
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());

  // Newest run per task, so a finished task can be opened from the board
  // instead of being hunted for under Runs.
  const runForTask = useMemo(() => {
    const map = new Map<string, (typeof runs)[number]>();
    for (const run of runs) {
      const seen = map.get(run.task_id);
      if (!seen || run.started_at > seen.started_at) map.set(run.task_id, run);
    }
    return map;
  }, [runs]);

  const byEpic = useMemo(() => {
    const map = new Map<string, TaskView[]>();
    for (const view of board?.tasks ?? []) {
      const key = view.task.epic_id ?? "__none";
      const list = map.get(key) ?? [];
      list.push(view);
      map.set(key, list);
    }
    for (const list of map.values()) {
      list.sort(
        (a, b) =>
          (a.task.priority ?? 1) - (b.task.priority ?? 1) ||
          a.task.title.localeCompare(b.task.title),
      );
    }
    return map;
  }, [board]);

  if (!board) {
    return (
      <div className="grid h-full place-items-center">
        {boardLoading ? <Spinner className="size-5 text-[var(--text-faint)]" /> : (
          <EmptyState
            title="No project selected"
            description="Pick a project from the sidebar to see its epics and tasks."
          />
        )}
      </div>
    );
  }

  const readyCount = board.ready.length;
  const activeHere = runs.filter(
    (r) => r.project_id === board.project.id && (r.status === "running" || r.status === "queued"),
  ).length;

  async function chooseFolder() {
    const path = await api.chooseFolder();
    if (!path || !board) return;
    await saveBinding({
      project_id: board.project.id,
      source: board.project.source ?? "flux",
      repo_path: path,
      base_branch: null,
      isolation: "worktree",
      integration: "leave",
      pipeline: { plan: false, review: true, document: false, max_revisions: 2 },
      roles: {},
      auto_run: false,
      max_parallel: 2,
      ...(binding ?? {}),
      ...(binding ? { repo_path: path } : {}),
    });
  }

  return (
    <div className="flex h-full flex-col">
      <header
        data-tauri-drag-region="deep"
        className="flex h-14 shrink-0 items-center gap-3 border-b px-5"
      >
        <div className="min-w-0 flex-1">
          <h1 className="truncate text-[15px] font-semibold tracking-tight">
            {board.project.name}
          </h1>
          <p className="truncate text-[11.5px] text-[var(--text-muted)]">
            {count(board.epics.length, "epic")} ·{" "}
            {count(board.tasks.length, "task")} · {readyCount} ready
            {activeHere > 0 && ` · ${activeHere} running`}
          </p>
        </div>

        <Button
          size="sm"
          variant="ghost"
          icon={<IconRefresh className="size-3.5" />}
          onClick={() => void refreshBoard()}
          title="Refresh from Flux"
        >
          Refresh
        </Button>
        <Button
          size="sm"
          variant="primary"
          icon={<IconSparks className="size-3.5" />}
          disabled={readyCount === 0 || !binding}
          onClick={() => void runReady()}
          title={
            !binding
              ? "Set this project's folder first"
              : readyCount === 0
                ? "Nothing is ready to run"
                : "Start the highest-priority ready task"
          }
        >
          Run next
        </Button>
      </header>

      <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
        <RepoStrip
          binding={binding}
          onChoose={() => void chooseFolder()}
          onToggleAuto={(auto) =>
            binding && void saveBinding({ ...binding, auto_run: auto })
          }
        />

        {!settings && <Spinner />}

        <div className="mt-4 flex flex-col gap-3">
          {board.epics.map((epic) => (
            <EpicSection
              key={epic.id}
              epic={epic}
              tasks={byEpic.get(epic.id) ?? []}
              collapsed={collapsed.has(epic.id)}
              hasBinding={Boolean(binding)}
              onToggleCollapse={() =>
                setCollapsed((prev) => {
                  const next = new Set(prev);
                  next.has(epic.id) ? next.delete(epic.id) : next.add(epic.id);
                  return next;
                })
              }
              onToggleAuto={(auto) => void setEpicAuto(epic.id, auto)}
              onRun={(taskId) => void startTask(taskId)}
              runForTask={runForTask}
              onOpenRun={openRun}
            />
          ))}

          {(byEpic.get("__none")?.length ?? 0) > 0 && (
            <Panel
              title="Not in an epic"
              description="Autonomy is granted per epic, so these can only be started by hand."
            >
              <TaskList
                tasks={byEpic.get("__none") ?? []}
                hasBinding={Boolean(binding)}
                onRun={(taskId) => void startTask(taskId)}
                runForTask={runForTask}
                onOpenRun={openRun}
              />
            </Panel>
          )}
        </div>
      </div>
    </div>
  );
}

function RepoStrip({
  binding,
  onChoose,
  onToggleAuto,
}: {
  binding: ReturnType<typeof currentBinding>;
  onChoose: () => void;
  onToggleAuto: (auto: boolean) => void;
}) {
  if (!binding) {
    return (
      <div
        className="flex items-center gap-3 rounded-xl border px-4 py-3"
        style={{ background: "var(--warn-soft)", borderColor: "transparent" }}
      >
        <IconAlert className="size-4 shrink-0" style={{ color: "var(--warn)" }} />
        <div className="min-w-0 flex-1">
          <p className="text-[12.5px] font-medium">This project has no folder yet</p>
          <p className="text-[11.5px] text-[var(--text-muted)]">
            Choose where it lives on this machine before agents can work on it.
          </p>
        </div>
        <Button size="sm" icon={<IconFolder className="size-3.5" />} onClick={onChoose}>
          Choose folder
        </Button>
      </div>
    );
  }

  return (
    <div
      className="flex flex-wrap items-center gap-x-4 gap-y-2 rounded-xl border px-4 py-2.5"
      style={{ background: "var(--surface)" }}
    >
      <button
        onClick={onChoose}
        className="flex min-w-0 items-center gap-2 text-left"
        title="Change folder"
      >
        <IconFolder className="size-4 shrink-0 text-[var(--text-faint)]" />
        <span className="truncate font-mono text-[11.5px]">{binding.repo_path}</span>
      </button>

      <div className="flex items-center gap-1.5 text-[11.5px] text-[var(--text-muted)]">
        <IconBranch className="size-3.5" />
        {binding.base_branch ?? "current branch"}
      </div>

      <Badge tone="neutral" title="Where agents run">
        {binding.isolation === "worktree" ? "Worktree per task" : "In place"}
      </Badge>

      <Badge tone="neutral">
        {binding.pipeline.plan && "Plan · "}Implement
        {binding.pipeline.review && " · Review"}
        {binding.pipeline.document && " · Document"}
      </Badge>

      <div className="ml-auto flex items-center gap-2">
        <span className="text-[11.5px] text-[var(--text-muted)]">
          Auto-run up to {binding.max_parallel}
        </span>
        <Toggle
          checked={binding.auto_run}
          onChange={onToggleAuto}
          label="Auto-run this project"
        />
      </div>
    </div>
  );
}

function EpicSection({
  epic,
  tasks,
  collapsed,
  hasBinding,
  onToggleCollapse,
  onToggleAuto,
  onRun,
  runForTask,
  onOpenRun,
}: {
  epic: Epic;
  tasks: TaskView[];
  collapsed: boolean;
  hasBinding: boolean;
  onToggleCollapse: () => void;
  onToggleAuto: (auto: boolean) => void;
  onRun: (taskId: string) => void;
  runForTask: Map<string, RunRecord>;
  onOpenRun: (runId: string) => void;
}) {
  const ready = tasks.filter((t) => t.ineligible === null).length;
  const done = tasks.filter((t) => t.task.status === "done").length;

  return (
    <section
      style={{
        background: "var(--surface)",
        boxShadow: "var(--shadow-panel)",
        // A tint rather than a full-strength accent: enough to pick the
        // auto-enabled epics out of a long board without shouting.
        borderColor: epic.auto ? "color-mix(in srgb, var(--accent) 45%, var(--border))" : undefined,
      }}
      className="rounded-xl border"
    >
      <header className="flex items-center gap-3 px-4 py-3">
        <button
          onClick={onToggleCollapse}
          className="text-[var(--text-faint)] transition-transform hover:text-[var(--text)]"
          style={{ transform: collapsed ? "rotate(0deg)" : "rotate(90deg)" }}
          aria-label={collapsed ? "Expand epic" : "Collapse epic"}
        >
          <IconChevron className="size-4" />
        </button>

        <div className="min-w-0 flex-1">
          <h2 className="truncate text-[13.5px] font-semibold">{epic.title}</h2>
          <p className="truncate text-[11.5px] text-[var(--text-muted)]">
            {count(tasks.length, "task")} · {ready} ready · {done} done
          </p>
        </div>

        {epic.depends_on.length > 0 && (
          <Badge tone="info" title="This epic waits on another epic">
            Depends on {epic.depends_on.length}
          </Badge>
        )}

        <div className="flex items-center gap-2">
          <span
            className="text-[11.5px] font-medium"
            style={{ color: epic.auto ? "var(--accent-text)" : "var(--text-faint)" }}
          >
            Auto
          </span>
          <Toggle
            checked={epic.auto}
            onChange={onToggleAuto}
            label={`Auto for ${epic.title}`}
          />
        </div>
      </header>

      {!collapsed && (
        <TaskList
          tasks={tasks}
          hasBinding={hasBinding}
          onRun={onRun}
          runForTask={runForTask}
          onOpenRun={onOpenRun}
        />
      )}
    </section>
  );
}

function TaskList({
  tasks,
  hasBinding,
  onRun,
  runForTask,
  onOpenRun,
}: {
  tasks: TaskView[];
  hasBinding: boolean;
  onRun: (taskId: string) => void;
  runForTask: Map<string, RunRecord>;
  onOpenRun: (runId: string) => void;
}) {
  if (tasks.length === 0) {
    return (
      <p className="border-t px-4 py-3 text-[12px] text-[var(--text-faint)]">
        No tasks in this epic.
      </p>
    );
  }

  return (
    <ul className="border-t">
      {tasks.map(({ task, ineligible }) => {
        const priority = (task.priority ?? 1) as Priority;
        const runnable = ineligible === null;
        const run = runForTask.get(task.id);

        return (
          <li
            key={task.id}
            className={cx(
              "group flex items-start gap-3 border-b px-4 py-2.5 last:border-b-0",
              run && "cursor-pointer hover:bg-[var(--surface-2)]",
            )}
            onClick={run ? () => onOpenRun(run.id) : undefined}
            title={run ? "Open this run" : undefined}
          >
            <span className="mt-1.5">
              <Dot
                tone={
                  task.status === "done"
                    ? "success"
                    : task.status === "in_progress"
                      ? "accent"
                      : runnable
                        ? "info"
                        : "neutral"
                }
                pulse={task.status === "in_progress"}
              />
            </span>

            <div className="min-w-0 flex-1">
              <p
                className={cx(
                  "text-[13px] leading-snug",
                  task.status === "done" && "text-[var(--text-muted)] line-through",
                )}
              >
                {task.title}
              </p>

              <div className="mt-1 flex flex-wrap items-center gap-1.5">
                <Badge tone={PRIORITY_TONE[priority]}>{PRIORITY_LABELS[priority]}</Badge>

                {task.acceptance_criteria.length > 0 && (
                  <Badge title="Acceptance criteria">
                    {count(
                      task.acceptance_criteria.length,
                      "criterion",
                      "criteria",
                    )}
                  </Badge>
                )}

                {task.guardrails.length > 0 && (
                  <Badge
                    tone="warn"
                    title={task.guardrails
                      .slice()
                      .sort((a, b) => b.number - a.number)
                      .map((g) => `${g.number}: ${g.text}`)
                      .join("\n")}
                  >
                    {count(task.guardrails.length, "guardrail")}
                  </Badge>
                )}

                {task.workers.length > 0 && (
                  <Badge tone="accent">{task.workers.join(", ")}</Badge>
                )}

                {run && (
                  <Badge
                    tone={
                      run.landing === "on_branch"
                        ? "warn"
                        : run.status === "succeeded"
                          ? "success"
                          : "neutral"
                    }
                    title={
                      run.landing === "on_branch"
                        ? `Committed to ${run.branch}, not yet merged`
                        : "Open this run"
                    }
                  >
                    {run.landing === "on_branch"
                      ? "Not merged"
                      : run.landing === "merged"
                        ? "Merged"
                        : "View run"}
                  </Badge>
                )}

                {task.blocked_reason && (
                  <Badge tone="danger" title={task.blocked_reason}>
                    {task.blocked_reason}
                  </Badge>
                )}

                {/* Finished work needs no explanation — the strikethrough
                    already says it. */}
                {!runnable && !task.blocked_reason && task.status !== "done" && (
                  <span className="text-[11px] text-[var(--text-faint)]">
                    {ineligible}
                  </span>
                )}
              </div>
            </div>

            {/* Finished work offers no action, so it gets no control. */}
            {task.status !== "done" && (
              <Button
                size="sm"
                variant={runnable ? "secondary" : "ghost"}
                icon={<IconPlay className="size-3" />}
                disabled={!hasBinding || task.status === "in_progress"}
                onClick={(event) => {
                  event.stopPropagation();
                  onRun(task.id);
                }}
                className={cx(!runnable && "opacity-0 group-hover:opacity-100")}
                title={
                  !hasBinding
                    ? "Set this project's folder first"
                    : runnable
                      ? "Start this task"
                      : `${ineligible} — start it anyway`
                }
              >
                Run
              </Button>
            )}
          </li>
        );
      })}
    </ul>
  );
}
