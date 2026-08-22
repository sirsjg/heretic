import { useEffect, useRef } from "react";
import { useStore } from "../lib/store";
import type {
  AgentEvent,
  RunFeedItem,
  RunRecord,
  RunStage,
  RunStatus,
} from "../lib/types";
import { STAGE_LABELS } from "../lib/types";
import { Badge, Button, Dot, EmptyState, Spinner, cx } from "../components/ui";
import {
  IconBranch,
  IconCheck,
  IconClock,
  IconClose,
  IconFolder,
  IconStop,
} from "../components/icons";
import { api } from "../lib/bridge";

const STATUS_TONE: Record<RunStatus, "accent" | "success" | "danger" | "warn" | "neutral"> = {
  queued: "neutral",
  running: "accent",
  succeeded: "success",
  failed: "danger",
  cancelled: "neutral",
  needs_attention: "warn",
};

const STATUS_LABEL: Record<RunStatus, string> = {
  queued: "Queued",
  running: "Running",
  succeeded: "Completed",
  failed: "Failed",
  cancelled: "Stopped",
  needs_attention: "Needs attention",
};

/** Stages shown in the timeline, in the order they happen. */
const TIMELINE: RunStage[] = [
  "planning",
  "implementing",
  "reviewing",
  "documenting",
  "integrating",
];

export function RunView() {
  const { runs, selectedRunId, openRun, stopRun, dismissRun } = useStore();
  const run = runs.find((r) => r.id === selectedRunId) ?? runs[0];

  if (!run) {
    return (
      <div className="grid h-full place-items-center">
        <EmptyState
          title="Nothing has run yet"
          description="Start a task from a board, or turn on Auto for an epic and let Accelerate pick up work as it becomes ready."
        />
      </div>
    );
  }

  return (
    <div className="flex h-full">
      <div className="flex min-w-0 flex-1 flex-col">
        <RunHeader
          run={run}
          onStop={() => void stopRun(run.id)}
          onDismiss={() => void dismissRun(run.id)}
        />
        <StageTimeline run={run} />
        <Feed run={run} />
      </div>

      {runs.length > 1 && (
        <aside
          className="w-64 shrink-0 overflow-y-auto border-l"
          style={{ background: "var(--surface)" }}
        >
          <p className="px-3 pb-1 pt-3 text-[10.5px] font-semibold uppercase tracking-wider text-[var(--text-faint)]">
            Runs
          </p>
          {runs.map((item) => (
            <button
              key={item.id}
              onClick={() => openRun(item.id)}
              style={
                item.id === run.id ? { background: "var(--accent-soft)" } : undefined
              }
              className={cx(
                "flex w-full flex-col gap-1 border-b px-3 py-2.5 text-left transition-colors",
                item.id !== run.id && "hover:bg-[var(--surface-3)]",
              )}
            >
              <span className="flex items-center gap-1.5">
                <Dot
                  tone={STATUS_TONE[item.status]}
                  pulse={item.status === "running"}
                />
                <span className="truncate text-[11.5px] text-[var(--text-muted)]">
                  {item.project_name}
                </span>
              </span>
              <span className="line-clamp-2 text-[12.5px] leading-snug">
                {item.task_title}
              </span>
            </button>
          ))}
        </aside>
      )}
    </div>
  );
}

function RunHeader({
  run,
  onStop,
  onDismiss,
}: {
  run: RunRecord;
  onStop: () => void;
  onDismiss: () => void;
}) {
  const active = run.status === "running" || run.status === "queued";

  return (
    <header data-tauri-drag-region="deep" className="shrink-0 border-b px-5 py-3">
      <div className="flex items-start gap-3">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <Badge tone={STATUS_TONE[run.status]}>
              {active && <Spinner className="size-2.5" />}
              {STATUS_LABEL[run.status]}
            </Badge>
            <span className="truncate text-[11.5px] text-[var(--text-muted)]">
              {run.project_name}
              {run.epic_title && ` · ${run.epic_title}`}
            </span>
          </div>
          <h1 className="mt-1 truncate text-[15px] font-semibold tracking-tight">
            {run.task_title}
          </h1>
        </div>

        {active ? (
          <Button size="sm" variant="danger" icon={<IconStop className="size-3.5" />} onClick={onStop}>
            Stop
          </Button>
        ) : (
          <Button size="sm" variant="ghost" icon={<IconClose className="size-3.5" />} onClick={onDismiss}>
            Dismiss
          </Button>
        )}
      </div>

      <div className="mt-2 flex flex-wrap items-center gap-x-4 gap-y-1 text-[11.5px] text-[var(--text-muted)]">
        {run.agent && (
          <span className="flex items-center gap-1.5">
            <Dot tone="accent" pulse={active} />
            {run.agent}
          </span>
        )}

        {run.branch && (
          <span className="flex items-center gap-1.5 font-mono">
            <IconBranch className="size-3.5" />
            {run.branch}
          </span>
        )}

        {run.worktree_path && (
          <button
            className="flex items-center gap-1.5 font-mono hover:text-[var(--text)]"
            onClick={() => void api.revealPath(run.worktree_path!)}
            title="Reveal the worktree"
          >
            <IconFolder className="size-3.5" />
            {shortenPath(run.worktree_path)}
          </button>
        )}

        <span className="flex items-center gap-1.5">
          <IconClock className="size-3.5" />
          {formatDuration(run)}
        </span>

        {run.revisions > 0 && (
          <Badge tone="warn" title="Times the reviewer sent the work back">
            {run.revisions} revision{run.revisions === 1 ? "" : "s"}
          </Badge>
        )}

        {run.changes.files_changed > 0 && (
          <span className="font-mono">
            {run.changes.files_changed} files{" "}
            <span style={{ color: "var(--success)" }}>+{run.changes.insertions}</span>{" "}
            <span style={{ color: "var(--danger)" }}>−{run.changes.deletions}</span>
          </span>
        )}
      </div>

      {run.result && run.result.kind !== "completed" && "reason" in run.result && (
        <p
          className="mt-2 rounded-lg px-3 py-2 text-[12px] leading-snug"
          style={{
            background: run.status === "failed" ? "var(--danger-soft)" : "var(--warn-soft)",
            color: run.status === "failed" ? "var(--danger)" : "var(--warn)",
          }}
        >
          {run.result.reason}
        </p>
      )}
    </header>
  );
}

function StageTimeline({ run }: { run: RunRecord }) {
  const reached = new Set(run.feed.map((item) => item.stage));
  const currentIndex = TIMELINE.indexOf(run.stage);

  return (
    <div className="flex shrink-0 items-center gap-1 border-b px-5 py-2.5">
      {TIMELINE.map((stage, index) => {
        const isCurrent = stage === run.stage && run.status === "running";
        const isDone =
          reached.has(stage) && (currentIndex > index || run.status === "succeeded");
        const isSkipped = !reached.has(stage) && currentIndex > index;

        return (
          <div key={stage} className="flex items-center gap-1">
            <span
              className={cx(
                "flex items-center gap-1.5 rounded-md px-2 py-1 text-[11.5px] font-medium transition-colors",
              )}
              style={{
                background: isCurrent
                  ? "var(--accent-soft)"
                  : isDone
                    ? "var(--success-soft)"
                    : "transparent",
                color: isCurrent
                  ? "var(--accent-text)"
                  : isDone
                    ? "var(--success)"
                    : isSkipped
                      ? "var(--text-faint)"
                      : "var(--text-muted)",
                opacity: isSkipped ? 0.5 : 1,
              }}
            >
              {isDone ? (
                <IconCheck className="size-3" />
              ) : isCurrent ? (
                <Spinner className="size-2.5" />
              ) : (
                <span className="size-1.5 rounded-full bg-current opacity-40" />
              )}
              {STAGE_LABELS[stage]}
            </span>
            {index < TIMELINE.length - 1 && (
              <span className="h-px w-3 bg-[var(--border)]" />
            )}
          </div>
        );
      })}
    </div>
  );
}

function Feed({ run }: { run: RunRecord }) {
  const endRef = useRef<HTMLDivElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);

  // Follow the tail while the user is at the bottom; stop the moment they
  // scroll up to read something.
  useEffect(() => {
    if (pinned.current) endRef.current?.scrollIntoView({ block: "end" });
  }, [run.feed.length]);

  function onScroll() {
    const element = containerRef.current;
    if (!element) return;
    const distance =
      element.scrollHeight - element.scrollTop - element.clientHeight;
    pinned.current = distance < 40;
  }

  if (run.feed.length === 0) {
    return (
      <div className="grid flex-1 place-items-center">
        <EmptyState
          title="Waiting for the agent"
          description="Output appears here as soon as the agent starts working."
        />
      </div>
    );
  }

  return (
    <div
      ref={containerRef}
      onScroll={onScroll}
      className="min-h-0 flex-1 overflow-y-auto px-5 py-3"
    >
      <div className="flex flex-col gap-1.5">
        {collapse(run.feed).map((entry, index) => (
          <FeedRow
            key={index}
            stage={entry.item.stage}
            event={entry.item.event}
            repeats={entry.repeats}
          />
        ))}
        {run.status !== "running" && run.status !== "queued" && (
          <Outcome run={run} />
        )}
        <div ref={endRef} />
      </div>
    </div>
  );
}

/** What the run left behind, shown once it has finished. */
function Outcome({ run }: { run: RunRecord }) {
  const files = run.changes.files;

  return (
    <div
      className="enter mt-2 rounded-lg border px-3 py-2.5"
      style={{ background: "var(--surface-2)" }}
    >
      <div className="flex items-center gap-2">
        <Badge tone={STATUS_TONE[run.status]}>{STATUS_LABEL[run.status]}</Badge>
        {run.changes.files_changed > 0 ? (
          <span className="font-mono text-[11.5px] text-[var(--text-muted)]">
            {run.changes.files_changed} files{" "}
            <span style={{ color: "var(--success)" }}>+{run.changes.insertions}</span>{" "}
            <span style={{ color: "var(--danger)" }}>−{run.changes.deletions}</span>
          </span>
        ) : (
          <span className="text-[11.5px] text-[var(--text-muted)]">
            Nothing was changed.
          </span>
        )}
      </div>

      {files.length > 0 && (
        <ul className="mt-2 flex flex-col gap-0.5">
          {files.slice(0, 12).map((file) => (
            <li
              key={file}
              className="truncate font-mono text-[11.5px] text-[var(--text-muted)]"
            >
              {file}
            </li>
          ))}
          {files.length > 12 && (
            <li className="text-[11.5px] text-[var(--text-faint)]">
              and {files.length - 12} more
            </li>
          )}
        </ul>
      )}

      {run.branch && run.status === "succeeded" && (
        <p className="mt-2 text-[11.5px] text-[var(--text-muted)]">
          Committed to{" "}
          <span className="font-mono" style={{ color: "var(--accent-text)" }}>
            {run.branch}
          </span>
          .
        </p>
      )}
    </div>
  );
}

/** Consecutive identical lines, folded into one with a count. */
function collapse(
  feed: RunFeedItem[],
): { item: RunFeedItem; repeats: number }[] {
  const out: { item: RunFeedItem; repeats: number }[] = [];

  for (const item of feed) {
    const previous = out.at(-1);
    const same =
      previous &&
      previous.item.stage === item.stage &&
      JSON.stringify(previous.item.event) === JSON.stringify(item.event);

    if (same) previous.repeats += 1;
    else out.push({ item, repeats: 1 });
  }

  return out;
}

/**
 * A backend's own internal logging, as opposed to something the agent said.
 *
 * Codex writes tracing lines to stderr in `<timestamp> LEVEL target: message`
 * form. They are worth keeping, but they are not agent output and should not
 * shout.
 */
const INTERNAL_LOG =
  /^\d{4}-\d{2}-\d{2}T[\d:.]+Z?\s+(TRACE|DEBUG|INFO|WARN|ERROR)\s+\S+:/;

function FeedRow({
  stage,
  event,
  repeats = 1,
}: {
  stage: RunStage;
  event: AgentEvent;
  repeats?: number;
}) {
  const stageLabel = STAGE_LABELS[stage];

  if (event.type === "tool") {
    return (
      <div className="enter flex items-start gap-2.5">
        <StageTag label={stageLabel} />
        <span
          className="rounded-md px-1.5 py-0.5 font-mono text-[11.5px]"
          style={{ background: "var(--surface-3)", color: "var(--accent-text)" }}
        >
          {event.name}
        </span>
        {event.detail && (
          <span className="min-w-0 truncate font-mono text-[11.5px] text-[var(--text-muted)]">
            {event.detail}
          </span>
        )}
        <Repeats count={repeats} />
      </div>
    );
  }

  if (event.type === "error") {
    return (
      <div className="enter flex items-start gap-2.5">
        <StageTag label={stageLabel} />
        <p className="text-[12.5px] leading-relaxed" style={{ color: "var(--danger)" }}>
          {event.message}
        </p>
        <Repeats count={repeats} />
      </div>
    );
  }

  if (event.type === "result") {
    const verdict = readVerdict(event.text ?? "");
    return (
      <div
        className="enter my-1 rounded-lg border px-3 py-2"
        style={{ background: "var(--surface-2)" }}
      >
        <div className="flex items-center gap-2">
          <StageTag label={stageLabel} />
          {verdict && (
            <Badge tone={verdict === "approve" ? "success" : "warn"}>
              {verdict === "approve" ? "Approved" : "Changes requested"}
            </Badge>
          )}
          {typeof event.duration_ms === "number" && (
            <span className="text-[11px] text-[var(--text-faint)]">
              {Math.round(event.duration_ms / 1000)}s
            </span>
          )}
          {typeof event.cost_usd === "number" && (
            <span className="text-[11px] text-[var(--text-faint)]">
              ${event.cost_usd.toFixed(3)}
            </span>
          )}
        </div>
        {event.text && (
          <p className="mt-1 whitespace-pre-wrap text-[12.5px] leading-relaxed">
            {stripVerdict(event.text)}
          </p>
        )}
      </div>
    );
  }

  const text = event.text;
  const internal = event.type === "raw" && INTERNAL_LOG.test(text);

  return (
    <div className="enter flex items-start gap-2.5">
      <StageTag label={stageLabel} />
      <p
        className={cx(
          "min-w-0 whitespace-pre-wrap text-[12.5px] leading-relaxed",
          event.type === "raw" && "font-mono text-[11.5px] text-[var(--text-muted)]",
          internal && "text-[11px] text-[var(--text-faint)]",
        )}
      >
        {text}
      </p>
      <Repeats count={repeats} />
    </div>
  );
}

/** "x3" beside a line that arrived several times over. */
function Repeats({ count }: { count: number }) {
  if (count < 2) return null;
  return (
    <span
      className="mt-0.5 shrink-0 rounded px-1 text-[10.5px] font-medium"
      style={{ background: "var(--surface-3)", color: "var(--text-muted)" }}
      title={`This line appeared ${count} times in a row`}
    >
      ×{count}
    </span>
  );
}

function StageTag({ label }: { label: string }) {
  return (
    <span className="mt-0.5 w-16 shrink-0 text-right text-[10.5px] uppercase tracking-wide text-[var(--text-faint)]">
      {label}
    </span>
  );
}

function readVerdict(text: string): "approve" | "request_changes" | null {
  const matches = [...text.matchAll(/verdict:\s*(approve|request[_ ]changes)/gi)];
  const last = matches.at(-1);
  if (!last?.[1]) return null;
  return last[1].toLowerCase().startsWith("approve") ? "approve" : "request_changes";
}

function stripVerdict(text: string): string {
  return text.replace(/^\s*verdict:.*$/gim, "").trim();
}

function shortenPath(path: string): string {
  const parts = path.split("/");
  return parts.length > 3 ? `…/${parts.slice(-2).join("/")}` : path;
}

function formatDuration(run: RunRecord): string {
  const start = new Date(run.started_at).getTime();
  const end = run.finished_at ? new Date(run.finished_at).getTime() : Date.now();
  const seconds = Math.max(0, Math.round((end - start) / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ${seconds % 60}s`;
  return `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}
