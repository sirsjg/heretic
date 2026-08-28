import { useCallback, useEffect, useRef, useState } from "react";
import { useStore } from "../lib/store";
import type {
  AgentEvent,
  RunFeedItem,
  RunRecord,
  RunStage,
  RunStatus,
} from "../lib/types";
import { STAGE_LABELS, isActive } from "../lib/types";
import { Badge, Button, Dot, EmptyState, Spinner, cx } from "../components/ui";
import { RunStats } from "../components/RunStats";
import {
  IconBranch,
  IconCheck,
  IconChevron,
  IconClock,
  IconClose,
  IconFileDiff,
  IconFolder,
  IconPanel,
  IconStop,
} from "../components/icons";
import { RunRail, Resizer, useRunShortcuts } from "../components/RunRail";
import { RunInspector, type InspectorTab } from "../components/RunInspector";
import { api } from "../lib/bridge";

const STATUS_TONE: Record<RunStatus, "accent" | "success" | "danger" | "warn" | "neutral"> = {
  queued: "neutral",
  running: "accent",
  waiting: "warn",
  succeeded: "success",
  failed: "danger",
  cancelled: "neutral",
  needs_attention: "warn",
};

const STATUS_LABEL: Record<RunStatus, string> = {
  queued: "Queued",
  running: "Running",
  waiting: "Waiting for you",
  succeeded: "Completed",
  failed: "Failed",
  cancelled: "Stopped",
  needs_attention: "Needs attention",
};

/** Panel sizes. The rail is fixed; the inspector is dragged and remembered. */
const RAIL_WIDTH = 236;
const INSPECTOR_DEFAULT = 420;
const INSPECTOR_MIN = 300;
const INSPECTOR_MAX = 900;

/** Stages shown in the timeline, in the order they happen. */
const TIMELINE: RunStage[] = [
  "planning",
  "implementing",
  "reviewing",
  "documenting",
  "integrating",
];

export function RunView() {
  const {
    runs,
    selectedRunId,
    openRun,
    stopRun,
    dismissRun,
    integrateRun,
    discardRunWork,
    answerQuestion,
  } = useStore();
  const run = runs.find((r) => r.id === selectedRunId) ?? runs[0];

  useRunShortcuts(runs, openRun);

  // The panel layout is a working preference, not application state: it belongs
  // to this machine and should survive a restart without going near the engine.
  const [inspectorOpen, setInspectorOpen] = useState(
    () => remembered("heretic.runs.inspector.open", "1") === "1",
  );
  const [inspectorWidth, setInspectorWidth] = useState(() =>
    Number(remembered("heretic.runs.inspector.width", String(INSPECTOR_DEFAULT))),
  );
  const [tab, setTab] = useState<InspectorTab>(() =>
    remembered("heretic.runs.inspector.tab", "changes") === "history"
      ? "history"
      : "changes",
  );

  const setWidth = useCallback((next: number) => {
    const clamped = Math.min(INSPECTOR_MAX, Math.max(INSPECTOR_MIN, Math.round(next)));
    setInspectorWidth(clamped);
    remember("heretic.runs.inspector.width", String(clamped));
  }, []);

  const showInspector = useCallback((open: boolean) => {
    setInspectorOpen(open);
    remember("heretic.runs.inspector.open", open ? "1" : "0");
  }, []);

  const openTab = useCallback(
    (next: InspectorTab) => {
      setTab(next);
      remember("heretic.runs.inspector.tab", next);
      showInspector(true);
    },
    [showInspector],
  );

  if (!run) {
    return (
      <div className="grid h-full place-items-center">
        <EmptyState
          title="Nothing has run yet"
          description="Start a task from a board, or turn on Auto for an epic and let Heretic pick up work as it becomes ready."
        />
      </div>
    );
  }

  return (
    <div className="flex h-full">
      {runs.length > 1 && (
        <RunRail
          runs={runs}
          selectedId={run.id}
          onSelect={openRun}
          width={RAIL_WIDTH}
        />
      )}

      <div className="flex min-w-[340px] flex-1 flex-col">
        <RunHeader
          run={run}
          inspectorOpen={inspectorOpen}
          onToggleInspector={() => showInspector(!inspectorOpen)}
          onStop={() => void stopRun(run.id)}
          onDismiss={() => void dismissRun(run.id)}
        />
        <StageTimeline run={run} />
        <Feed
          run={run}
          onReview={() => openTab("changes")}
          onIntegrate={() => void integrateRun(run.id)}
          onDiscard={() => void discardRunWork(run.id)}
          onAnswer={(answer) => void answerQuestion(run.id, answer)}
        />
      </div>

      {inspectorOpen && (
        <>
          <Resizer
            width={inspectorWidth}
            onWidth={setWidth}
            onReset={() => setWidth(INSPECTOR_DEFAULT)}
            invert
          />
          <RunInspector
            run={run}
            tab={tab}
            onTab={openTab}
            onClose={() => showInspector(false)}
            width={inspectorWidth}
          />
        </>
      )}
    </div>
  );
}

/** Read a remembered layout preference, tolerating a browser that has none. */
function remembered(key: string, fallback: string): string {
  try {
    return localStorage.getItem(key) ?? fallback;
  } catch {
    return fallback;
  }
}

function remember(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Private windows and locked-down webviews refuse; the layout still works.
  }
}

function RunHeader({
  run,
  inspectorOpen,
  onToggleInspector,
  onStop,
  onDismiss,
}: {
  run: RunRecord;
  inspectorOpen: boolean;
  onToggleInspector: () => void;
  onStop: () => void;
  onDismiss: () => void;
}) {
  const active = isActive(run);
  useClock(active);

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

        <button
          onClick={onToggleInspector}
          aria-pressed={inspectorOpen}
          title={inspectorOpen ? "Hide the changes panel" : "Show the changes panel"}
          style={inspectorOpen ? { color: "var(--accent-text)" } : undefined}
          className={cx(
            "mt-0.5 shrink-0 rounded-md p-1.5",
            !inspectorOpen && "text-[var(--text-faint)]",
            "hover:bg-[var(--surface-3)] hover:text-[var(--text)]",
          )}
        >
          <IconPanel className="size-4" />
        </button>

        {active ? (
          <Button size="sm" variant="danger" icon={<IconStop className="size-3.5" />} onClick={onStop}>
            Stop
          </Button>
        ) : (
          <Button
            size="sm"
            variant="ghost"
            icon={<IconClose className="size-3.5" />}
            onClick={onDismiss}
            title={
              run.landing === "on_branch"
                ? `Removes this run from the list. ${run.branch} and its worktree stay on disk.`
                : "Remove this run from the list"
            }
          >
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

        {run.landing === "on_branch" && (
          <Badge tone="warn" title="Committed to its branch, but not merged">
            Not merged
          </Badge>
        )}

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
  const inFlight = run.status === "running" || run.status === "waiting";

  return (
    <div className="flex shrink-0 items-center gap-1 border-b px-5 py-2.5">
      {TIMELINE.map((stage, index) => {
        const isCurrent = stage === run.stage && inFlight;
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

function Feed({
  run,
  onReview,
  onIntegrate,
  onDiscard,
  onAnswer,
}: {
  run: RunRecord;
  onReview: () => void;
  onIntegrate: () => void;
  onDiscard: () => void;
  onAnswer: (answer: string) => void;
}) {
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
        {run.status === "waiting" && run.question && (
          <AnswerBox question={run.question.question} onAnswer={onAnswer} />
        )}
        {!isActive(run) && (
          <>
            <Outcome
              run={run}
              onReview={onReview}
              onIntegrate={onIntegrate}
              onDiscard={onDiscard}
            />
            <RunStats run={run} />
          </>
        )}
        <div ref={endRef} />
      </div>
    </div>
  );
}

/** What the run left behind, shown once it has finished. */
function Outcome({
  run,
  onReview,
  onIntegrate,
  onDiscard,
}: {
  run: RunRecord;
  onReview: () => void;
  onIntegrate: () => void;
  onDiscard: () => void;
}) {
  const changed = run.changes.files_changed;

  return (
    <div
      className="enter mt-2 rounded-lg border px-3 py-2.5"
      style={{ background: "var(--surface-2)" }}
    >
      <div className="flex flex-wrap items-center gap-2">
        <Badge tone={STATUS_TONE[run.status]}>{STATUS_LABEL[run.status]}</Badge>
        {changed > 0 ? (
          <>
            <span className="font-mono text-[11.5px] text-[var(--text-muted)]">
              {changed} file{changed === 1 ? "" : "s"}{" "}
              <span style={{ color: "var(--success)" }}>+{run.changes.insertions}</span>{" "}
              <span style={{ color: "var(--danger)" }}>−{run.changes.deletions}</span>
            </span>
            {/* The diff itself lives in the panel beside the feed, where there
                is room to read it. This is the way in. */}
            <Button
              size="sm"
              variant="secondary"
              icon={<IconFileDiff className="size-3.5" />}
              onClick={onReview}
              className="ml-auto"
            >
              Read the diff
            </Button>
          </>
        ) : (
          <span className="text-[11.5px] text-[var(--text-muted)]">
            Nothing was changed.
          </span>
        )}
      </div>

      {run.landing === "on_branch" && run.branch && (
        <div className="mt-2 flex flex-wrap items-center gap-2 border-t pt-2">
          <p className="min-w-0 flex-1 text-[11.5px] leading-snug text-[var(--text-muted)]">
            Committed to{" "}
            <span className="font-mono" style={{ color: "var(--accent-text)" }}>
              {run.branch}
            </span>
            , in its own worktree. Nothing is on{" "}
            <span className="font-mono">{run.base_branch ?? "the base branch"}</span>{" "}
            until you merge it.
          </p>
          <Button size="sm" variant="primary" onClick={onIntegrate}>
            Merge into {run.base_branch ?? "base"}
          </Button>
          <Button size="sm" variant="ghost" onClick={onDiscard}>
            Discard
          </Button>
        </div>
      )}

      {run.landing === "merged" && (
        <p className="mt-2 text-[11.5px] text-[var(--text-muted)]">
          Merged into{" "}
          <span className="font-mono" style={{ color: "var(--success)" }}>
            {run.base_branch ?? "the base branch"}
          </span>
          , and the worktree has been removed.
        </p>
      )}

      {run.landing === "discarded" && (
        <p className="mt-2 text-[11.5px] text-[var(--text-muted)]">
          Discarded — branch deleted and worktree removed.
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

  // Token accounting belongs to the stats panel, not the conversation.
  if (event.type === "usage") return null;

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

  if (event.type === "prompt") {
    return <PromptBlock stage={stage} text={event.text} />;
  }

  if (event.type === "question") {
    return (
      <div
        className="enter my-1 rounded-lg border px-3 py-2"
        style={{ background: "var(--warn-soft)" }}
      >
        <div className="flex items-center gap-2">
          <StageTag label={stageLabel} />
          <Badge tone="warn">Question for you</Badge>
        </div>
        <p className="mt-1 whitespace-pre-wrap text-[12.5px] leading-relaxed">
          {event.text}
        </p>
      </div>
    );
  }

  if (event.type === "answer") {
    return (
      <div
        className="enter my-1 rounded-lg border px-3 py-2"
        style={{ background: "var(--accent-soft)" }}
      >
        <div className="flex items-center gap-2">
          <StageTag label={stageLabel} />
          <Badge tone="accent">Your answer</Badge>
        </div>
        <p className="mt-1 whitespace-pre-wrap text-[12.5px] leading-relaxed">
          {event.text}
        </p>
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

/**
 * The prompt Heretic built for a stage, folded away until asked for.
 *
 * This is the hand-off made visible — the planner's brief reaching the
 * implementer, the diff and the review notes reaching it back — and it is the
 * one thing the command line above it deliberately hides. Collapsed by default
 * because a reviewer prompt carries the whole diff and would bury the run.
 */
function PromptBlock({ stage, text }: { stage: RunStage; text: string }) {
  const [open, setOpen] = useState(false);
  const words = text.trim() ? text.trim().split(/\s+/).length : 0;

  return (
    <div
      className="enter my-1 rounded-lg border"
      style={{ background: "var(--surface-2)" }}
    >
      <button
        onClick={() => setOpen((value) => !value)}
        className="flex w-full items-center gap-2 px-3 py-1.5 text-left"
        title={open ? "Hide the prompt" : "Show the prompt this stage was given"}
      >
        <StageTag label={STAGE_LABELS[stage]} />
        <Badge tone="neutral">Generated prompt</Badge>
        <span className="text-[11px] text-[var(--text-faint)]">
          {words.toLocaleString()} words
        </span>
        <IconChevron
          className="ml-auto size-3.5 shrink-0 text-[var(--text-faint)] transition-transform"
          style={{ transform: open ? "rotate(90deg)" : undefined }}
        />
      </button>

      {open && (
        <div className="border-t px-3 py-2">
          <pre className="max-h-96 overflow-auto whitespace-pre-wrap break-words font-mono text-[11.5px] leading-relaxed text-[var(--text-muted)]">
            {text}
          </pre>
          <button
            onClick={() => void navigator.clipboard.writeText(text)}
            className="mt-1.5 text-[11px] text-[var(--text-faint)] hover:text-[var(--text)]"
          >
            Copy
          </button>
        </div>
      )}
    </div>
  );
}

/**
 * Where the user answers the question a run is paused on.
 *
 * The question itself is already in the feed just above; this is the reply.
 */
function AnswerBox({
  question,
  onAnswer,
}: {
  question: string;
  onAnswer: (answer: string) => void;
}) {
  const [draft, setDraft] = useState("");

  function send() {
    const answer = draft.trim();
    if (!answer) return;
    onAnswer(answer);
    setDraft("");
  }

  return (
    <div
      className="enter my-1 rounded-lg border px-3 py-2.5"
      style={{ background: "var(--surface-2)", borderColor: "var(--warn)" }}
    >
      <p className="text-[12px] font-medium">The run is paused on this question:</p>
      <p className="mt-1 whitespace-pre-wrap text-[12.5px] leading-relaxed text-[var(--text-muted)]">
        {question}
      </p>
      <div className="mt-2 flex items-end gap-2">
        <textarea
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
              e.preventDefault();
              send();
            }
          }}
          rows={2}
          placeholder="Type your answer… (⌘↵ to send)"
          className="min-h-[3.25rem] flex-1 resize-y rounded-lg border px-2.5 py-1.5 text-[12.5px] leading-relaxed outline-none focus:border-[var(--accent)]"
          style={{ background: "var(--surface)" }}
          autoFocus
        />
        <Button size="sm" variant="primary" onClick={send} disabled={!draft.trim()}>
          Answer
        </Button>
      </div>
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

/**
 * Re-render once a second while `active`, so a duration measured against
 * `Date.now()` actually ticks instead of waiting for the next engine event.
 */
function useClock(active: boolean) {
  const [, setTick] = useState(0);
  useEffect(() => {
    if (!active) return;
    const id = setInterval(() => setTick((tick) => tick + 1), 1000);
    return () => clearInterval(id);
  }, [active]);
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
