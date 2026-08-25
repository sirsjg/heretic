/**
 * The list of runs, down the left of the runs screen.
 *
 * Runs are grouped by project because that is how they are thought about — "what
 * is happening on Corporate Travel" — and each row carries the two things worth
 * knowing at a glance: how much it changed, and whether it still needs a
 * decision from you. The first nine rows answer to ⌘1…⌘9, so switching between
 * runs never needs the mouse.
 */

import { useEffect, useMemo } from "react";
import type { Landing, RunRecord, RunStatus } from "../lib/types";
import { Dot, cx } from "./ui";

const STATUS_TONE: Record<RunStatus, "accent" | "success" | "danger" | "warn" | "neutral"> = {
  queued: "neutral",
  running: "accent",
  succeeded: "success",
  failed: "danger",
  cancelled: "neutral",
  needs_attention: "warn",
};

/**
 * The one line under a run's title: what it is waiting for, or what became of
 * it. Deliberately about the decision, not the mechanics.
 */
function standing(run: RunRecord): { text: string; tone: string } {
  if (run.status === "running") return { text: "Working", tone: "var(--accent-text)" };
  if (run.status === "queued") return { text: "Queued", tone: "var(--text-faint)" };
  if (run.status === "failed") return { text: "Failed", tone: "var(--danger)" };
  if (run.status === "cancelled") return { text: "Stopped", tone: "var(--text-faint)" };
  if (run.status === "needs_attention")
    return { text: "Needs attention", tone: "var(--warn)" };

  const landed: Record<Landing, { text: string; tone: string }> = {
    on_branch: { text: "Ready to merge", tone: "var(--success)" },
    merged: { text: "Merged", tone: "var(--text-muted)" },
    discarded: { text: "Discarded", tone: "var(--text-faint)" },
    nothing: { text: "Nothing changed", tone: "var(--text-faint)" },
  };
  return landed[run.landing];
}

/**
 * The order runs are shown in, which is also the order ⌘1…⌘9 count in: newest
 * first, gathered by project.
 */
export function railOrder(runs: RunRecord[]): RunRecord[] {
  return group(runs).flatMap(([, items]) => items);
}

function group(runs: RunRecord[]): [string, RunRecord[]][] {
  const byProject = new Map<string, RunRecord[]>();
  for (const run of runs) {
    const existing = byProject.get(run.project_name);
    if (existing) existing.push(run);
    else byProject.set(run.project_name, [run]);
  }
  return [...byProject.entries()];
}

/** ⌘1…⌘9 (Ctrl elsewhere) jump straight to a run, in the order the rail shows. */
export function useRunShortcuts(
  runs: RunRecord[],
  onSelect: (runId: string) => void,
) {
  const ordered = useMemo(() => railOrder(runs), [runs]);

  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (!event.metaKey && !event.ctrlKey) return;
      if (event.altKey || event.shiftKey) return;

      const index = Number(event.key) - 1;
      if (!Number.isInteger(index) || index < 0 || index > 8) return;

      const run = ordered[index];
      if (!run) return;
      event.preventDefault();
      onSelect(run.id);
    }

    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [ordered, onSelect]);
}

export function RunRail({
  runs,
  selectedId,
  onSelect,
  width,
}: {
  runs: RunRecord[];
  selectedId: string | null;
  onSelect: (runId: string) => void;
  width: number;
}) {
  // Preserve the order the runs arrived in (newest first) while gathering each
  // project's runs together.
  const groups = useMemo(() => group(runs), [runs]);

  const ordered = useMemo(() => railOrder(runs), [runs]);

  return (
    <aside
      // Below this the feed has no room left to be read, and the run being
      // watched matters more than the list of the others. ⌘1…⌘9 still switch.
      className="flex shrink-0 flex-col overflow-y-auto border-r max-[1100px]:hidden"
      style={{ width, background: "var(--surface)" }}
    >
      {groups.map(([project, items]) => (
        <section key={project}>
          <h2 className="px-3 pb-1 pt-3 text-[11px] font-semibold text-[var(--text-muted)]">
            {project}
          </h2>

          {items.map((run) => (
            <RunRow
              key={run.id}
              run={run}
              selected={run.id === selectedId}
              shortcut={ordered.indexOf(run) + 1}
              onSelect={() => onSelect(run.id)}
            />
          ))}
        </section>
      ))}
      <div className="h-3" />
    </aside>
  );
}

function RunRow({
  run,
  selected,
  shortcut,
  onSelect,
}: {
  run: RunRecord;
  selected: boolean;
  shortcut: number;
  onSelect: () => void;
}) {
  const state = standing(run);
  const changed = run.changes.files_changed > 0;

  return (
    <button
      onClick={onSelect}
      aria-current={selected ? "true" : undefined}
      style={selected ? { background: "var(--accent-soft)" } : undefined}
      className={cx(
        "group flex w-full flex-col gap-1 border-b px-3 py-2 text-left transition-colors",
        !selected && "hover:bg-[var(--surface-3)]",
      )}
    >
      <span className="flex items-start gap-1.5">
        <span className="mt-[5px]">
          <Dot tone={STATUS_TONE[run.status]} pulse={run.status === "running"} />
        </span>
        <span className="line-clamp-2 min-w-0 flex-1 text-[12.5px] leading-snug">
          {run.task_title}
        </span>
        {changed && (
          <span
            className="mt-px shrink-0 rounded px-1 font-mono text-[10.5px] leading-[1.5]"
            style={{ background: "var(--surface-3)" }}
            title={`${run.changes.files_changed} files changed`}
          >
            <span style={{ color: "var(--success)" }}>+{run.changes.insertions}</span>{" "}
            <span style={{ color: "var(--danger)" }}>−{run.changes.deletions}</span>
          </span>
        )}
      </span>

      <span className="flex items-center gap-1.5 pl-3">
        <span className="truncate text-[11px]" style={{ color: state.tone }}>
          {state.text}
        </span>
        {run.revisions > 0 && (
          <span className="shrink-0 text-[11px] text-[var(--text-faint)]">
            · {run.revisions} revision{run.revisions === 1 ? "" : "s"}
          </span>
        )}
        {shortcut <= 9 && (
          <span
            className={cx(
              "ml-auto shrink-0 text-[10.5px] text-[var(--text-faint)]",
              !selected && "opacity-0 transition-opacity group-hover:opacity-100",
            )}
          >
            ⌘{shortcut}
          </span>
        )}
      </span>
    </button>
  );
}

/**
 * The handle between two panes.
 *
 * Drag to resize, double-click to put it back. `invert` is for a panel on the
 * right, where dragging left has to make the panel wider rather than narrower.
 */
export function Resizer({
  width,
  onWidth,
  onReset,
  invert,
}: {
  width: number;
  onWidth: (next: number) => void;
  onReset: () => void;
  invert?: boolean;
}) {
  return (
    <div
      role="separator"
      aria-orientation="vertical"
      title="Drag to resize, double-click to reset"
      onDoubleClick={onReset}
      onPointerDown={(event) => {
        event.preventDefault();
        const startX = event.clientX;
        const startWidth = width;

        const move = (moved: PointerEvent) => {
          const delta = moved.clientX - startX;
          onWidth(startWidth + (invert ? -delta : delta));
        };
        const stop = () => {
          window.removeEventListener("pointermove", move);
          window.removeEventListener("pointerup", stop);
          document.body.style.cursor = "";
          document.body.style.userSelect = "";
        };

        document.body.style.cursor = "col-resize";
        // Without this a drag across the feed selects every line it passes.
        document.body.style.userSelect = "none";
        window.addEventListener("pointermove", move);
        window.addEventListener("pointerup", stop);
      }}
      className={cx(
        "relative z-10 w-px shrink-0 cursor-col-resize bg-[var(--border)]",
        // A one-pixel line is honest but impossible to grab, so widen the
        // target without widening the line.
        "after:absolute after:inset-y-0 after:-left-1 after:-right-1 after:content-['']",
        "transition-colors hover:bg-[var(--accent)]",
      )}
    />
  );
}
