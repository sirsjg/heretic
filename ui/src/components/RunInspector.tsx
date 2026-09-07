/**
 * What a run did to the repository, beside what it said about it.
 *
 * The feed is the agent's account of the work; this is the work. Two tabs, and
 * both read straight from git: **Changes** is the run's whole diff against the
 * branch it forked from — every file, committed or not — and **History** is the
 * commits it actually made, each one openable as a patch.
 *
 * It stays useful after a merge. The engine falls back to reading the branch
 * from the project's own checkout once the worktree is gone, so a finished run
 * is still reviewable rather than becoming an empty panel and a memory.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api } from "../lib/bridge";
import { splitPath } from "../lib/diff";
import type { ChangeStatus, FileChange, RunCommit, RunRecord } from "../lib/types";
import { CHANGE_STATUS_LABELS, CHANGE_STATUS_LETTERS, isActive } from "../lib/types";
import { Badge, EmptyState, Spinner, cx } from "./ui";
import { Collapsible, DiffStat, PatchView } from "./DiffView";
import {
  IconAlert,
  IconChevron,
  IconClose,
  IconCopy,
  IconFileDiff,
  IconHistory,
  IconRefresh,
  IconSearch,
} from "./icons";

export type InspectorTab = "changes" | "history";

/** Narrower than this and a diff is not worth rendering at all. */
const INSPECTOR_FLOOR = 280;

/** The colour of a file's status letter, matching what the letter means. */
const STATUS_TONE: Record<ChangeStatus, string> = {
  added: "var(--success)",
  untracked: "var(--success)",
  modified: "var(--warn)",
  deleted: "var(--danger)",
  renamed: "var(--info)",
};

export function RunInspector({
  run,
  tab,
  onTab,
  onClose,
  width,
}: {
  run: RunRecord;
  tab: InspectorTab;
  onTab: (tab: InspectorTab) => void;
  onClose: () => void;
  width: number;
}) {
  // Bumped by the refresh button. Everything else re-reads on its own when the
  // run moves on, so this is only for "I changed something outside the app".
  const [nonce, setNonce] = useState(0);
  const refresh = useCallback(() => setNonce((value) => value + 1), []);

  return (
    <aside
      className="flex flex-col overflow-hidden border-l"
      // The chosen width is what it wants, not what it insists on: on a narrow
      // window it gives ground so the feed beside it stays readable.
      style={{
        flex: `0 1 ${width}px`,
        minWidth: INSPECTOR_FLOOR,
        background: "var(--surface)",
      }}
    >
      <header className="flex shrink-0 items-center gap-1 border-b px-2 py-1.5">
        <Tab
          active={tab === "changes"}
          onClick={() => onTab("changes")}
          icon={<IconFileDiff className="size-3.5" />}
          count={run.changes.files_changed}
        >
          Changes
        </Tab>
        <Tab
          active={tab === "history"}
          onClick={() => onTab("history")}
          icon={<IconHistory className="size-3.5" />}
        >
          History
        </Tab>

        <button
          onClick={refresh}
          title="Read the repository again"
          className="ml-auto rounded p-1 text-[var(--text-faint)] hover:bg-[var(--surface-3)] hover:text-[var(--text)]"
        >
          <IconRefresh className="size-3.5" />
        </button>
        <button
          onClick={onClose}
          title="Hide this panel"
          className="rounded p-1 text-[var(--text-faint)] hover:bg-[var(--surface-3)] hover:text-[var(--text)]"
        >
          <IconClose className="size-3.5" />
        </button>
      </header>

      {tab === "changes" ? (
        <ChangesTab run={run} nonce={nonce} />
      ) : (
        <HistoryTab run={run} nonce={nonce} />
      )}
    </aside>
  );
}

function Tab({
  active,
  onClick,
  icon,
  count,
  children,
}: {
  active: boolean;
  onClick: () => void;
  icon: React.ReactNode;
  count?: number;
  children: React.ReactNode;
}) {
  return (
    <button
      role="tab"
      aria-selected={active}
      onClick={onClick}
      style={active ? { background: "var(--surface-3)", color: "var(--text)" } : undefined}
      className={cx(
        "flex items-center gap-1.5 rounded-md px-2 py-1 text-[12px] font-medium",
        !active && "text-[var(--text-muted)] hover:text-[var(--text)]",
      )}
    >
      {icon}
      {children}
      {typeof count === "number" && count > 0 && (
        <span className="text-[11px] text-[var(--text-faint)]">{count}</span>
      )}
    </button>
  );
}

// --- Changes -----------------------------------------------------------------

function ChangesTab({ run, nonce }: { run: RunRecord; nonce: number }) {
  const [files, setFiles] = useState<FileChange[] | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const [filter, setFilter] = useState("");

  // A run's diff changes as it works, so re-read whenever the engine reports
  // that its summary moved — not on a timer, which would fight the scroll.
  const signature = `${run.id}:${run.status}:${run.landing}:${run.changes.files_changed}:${run.changes.insertions}:${run.changes.deletions}:${nonce}`;

  useEffect(() => {
    let live = true;
    setProblem(null);

    api
      .runChangedFiles(run.id)
      .then((result) => live && setFiles(result))
      .catch((error) => {
        if (!live) return;
        setFiles([]);
        setProblem(describe(error));
      });

    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [signature]);

  const shown = useMemo(() => {
    if (!files) return [];
    const needle = filter.trim().toLowerCase();
    if (!needle) return files;
    return files.filter((file) => file.path.toLowerCase().includes(needle));
  }, [files, filter]);

  if (problem) return <Problem message={problem} />;
  if (!files) return <Loading />;

  if (files.length === 0) {
    return (
      <EmptyState
        title="Nothing has changed yet"
        description={
          isActive(run)
            ? "Files appear here the moment the agent writes one."
            : "This run finished without touching the repository."
        }
      />
    );
  }

  const insertions = files.reduce((sum, file) => sum + file.insertions, 0);
  const deletions = files.reduce((sum, file) => sum + file.deletions, 0);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 items-center gap-2 border-b px-2.5 py-1.5">
        <IconSearch className="size-3.5 shrink-0 text-[var(--text-faint)]" />
        <input
          value={filter}
          onChange={(event) => setFilter(event.target.value)}
          placeholder="Filter files"
          className="min-w-0 flex-1 bg-transparent text-[12px] outline-none placeholder:text-[var(--text-faint)]"
        />
        <span className="shrink-0 font-mono text-[11px] text-[var(--text-faint)]">
          {files.length}
        </span>
        <DiffStat additions={insertions} deletions={deletions} />
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
        {shown.map((file) => (
          <FileRow
            key={file.path}
            runId={run.id}
            file={file}
            open={open === file.path}
            onToggle={() => setOpen(open === file.path ? null : file.path)}
          />
        ))}

        {shown.length === 0 && (
          <p className="px-2.5 py-3 text-[12px] text-[var(--text-faint)]">
            No file matches “{filter}”.
          </p>
        )}
      </div>
    </div>
  );
}

function FileRow({
  runId,
  file,
  open,
  onToggle,
}: {
  runId: string;
  file: FileChange;
  open: boolean;
  onToggle: () => void;
}) {
  const { directory, name } = splitPath(file.path);
  const [patch, setPatch] = useState<string | null>(null);
  const [problem, setProblem] = useState<string | null>(null);

  // Diffs are fetched the first time a file is opened, and kept afterwards:
  // opening one file should not cost a round trip for the other forty.
  const requested = useRef(false);

  useEffect(() => {
    if (!open || requested.current) return;
    requested.current = true;

    api
      .runFileDiff(runId, file.path)
      .then(setPatch)
      .catch((error) => setProblem(describe(error)));
  }, [open, runId, file.path]);

  const head = (
    <>
      <span
        aria-label={CHANGE_STATUS_LABELS[file.status]}
        title={CHANGE_STATUS_LABELS[file.status]}
        className="w-3 shrink-0 text-center font-mono text-[11px]"
        style={{ color: STATUS_TONE[file.status] }}
      >
        {CHANGE_STATUS_LETTERS[file.status]}
      </span>

      <span className="min-w-0 flex-1 truncate text-left font-mono text-[11.5px]">
        <span className="text-[var(--text-faint)]">{directory}</span>
        <span>{name}</span>
      </span>

      {file.binary ? (
        <span className="shrink-0 text-[10.5px] text-[var(--text-faint)]">binary</span>
      ) : (
        <DiffStat additions={file.insertions} deletions={file.deletions} />
      )}
    </>
  );

  return (
    <Collapsible open={open} onToggle={onToggle} head={head}>
      {file.old_path && (
        <p className="pb-1.5 font-mono text-[11px] text-[var(--text-faint)]">
          renamed from {file.old_path}
        </p>
      )}
      {problem ? (
        <p className="text-[11.5px]" style={{ color: "var(--danger)" }}>
          {problem}
        </p>
      ) : patch === null ? (
        <Spinner className="size-3.5 text-[var(--text-faint)]" />
      ) : (
        <PatchView
          patch={patch}
          emptyMessage={
            file.binary ? "Binary file — nothing to show." : "No textual changes."
          }
        />
      )}
    </Collapsible>
  );
}

// --- History -----------------------------------------------------------------

function HistoryTab({ run, nonce }: { run: RunRecord; nonce: number }) {
  const [commits, setCommits] = useState<RunCommit[] | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [open, setOpen] = useState<string | null>(null);

  const signature = `${run.id}:${run.status}:${run.landing}:${nonce}`;

  useEffect(() => {
    let live = true;
    setProblem(null);

    api
      .runCommits(run.id)
      .then((result) => live && setCommits(result))
      .catch((error) => {
        if (!live) return;
        setCommits([]);
        setProblem(describe(error));
      });

    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [signature]);

  if (problem) return <Problem message={problem} />;
  if (!commits) return <Loading />;

  if (commits.length === 0) {
    return (
      <EmptyState
        title="No commits yet"
        description={
          isActive(run)
            ? "Heretic commits the work when the run finishes, so this fills in at the end."
            : "This run left nothing committed on its branch."
        }
      />
    );
  }

  return (
    <div className="min-h-0 flex-1 overflow-y-auto">
      {run.branch && (
        <p className="border-b px-2.5 py-1.5 font-mono text-[11px] text-[var(--text-faint)]">
          {run.branch}
          {run.base_branch && ` ← ${run.base_branch}`}
        </p>
      )}

      {commits.map((commit) => (
        <CommitRow
          key={commit.sha}
          runId={run.id}
          commit={commit}
          open={open === commit.sha}
          onToggle={() => setOpen(open === commit.sha ? null : commit.sha)}
        />
      ))}
    </div>
  );
}

function CommitRow({
  runId,
  commit,
  open,
  onToggle,
}: {
  runId: string;
  commit: RunCommit;
  open: boolean;
  onToggle: () => void;
}) {
  const [patch, setPatch] = useState<string | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const requested = useRef(false);

  useEffect(() => {
    if (!open || requested.current) return;
    requested.current = true;

    api
      .runCommitDiff(runId, commit.sha)
      .then(setPatch)
      .catch((error) => setProblem(describe(error)));
  }, [open, runId, commit.sha]);

  return (
    <div className="border-b last:border-b-0">
      <button
        onClick={onToggle}
        className="flex w-full items-start gap-2 px-2.5 py-2 text-left hover:bg-[var(--surface-3)]"
      >
        <IconChevron
          className="mt-1 size-3 shrink-0 text-[var(--text-faint)] transition-transform"
          style={{ transform: open ? "rotate(90deg)" : undefined }}
        />

        <span className="min-w-0 flex-1">
          <span className="flex items-baseline gap-2">
            <span className="min-w-0 flex-1 truncate text-[12.5px]">
              {commit.subject}
            </span>
            <DiffStat additions={commit.insertions} deletions={commit.deletions} />
          </span>
          <span className="mt-0.5 flex items-center gap-1.5 text-[11px] text-[var(--text-faint)]">
            <span className="font-mono">{commit.short_sha}</span>
            <span>·</span>
            <span className="truncate">{commit.author}</span>
            <span>·</span>
            <span className="shrink-0">{when(commit.authored_at)}</span>
            <span>·</span>
            <span className="shrink-0">
              {commit.files_changed} file{commit.files_changed === 1 ? "" : "s"}
            </span>
          </span>
        </span>
      </button>

      {open && (
        <div className="px-2.5 pb-2.5">
          {commit.body && (
            <p className="mb-2 whitespace-pre-wrap text-[11.5px] leading-relaxed text-[var(--text-muted)]">
              {commit.body}
            </p>
          )}

          <button
            onClick={() => void navigator.clipboard.writeText(commit.sha)}
            title="Copy the full commit id"
            className="mb-2 inline-flex items-center gap-1 text-[11px] text-[var(--text-faint)] hover:text-[var(--text)]"
          >
            <IconCopy className="size-3" />
            {commit.sha}
          </button>

          {problem ? (
            <p className="text-[11.5px]" style={{ color: "var(--danger)" }}>
              {problem}
            </p>
          ) : patch === null ? (
            <Spinner className="size-3.5 text-[var(--text-faint)]" />
          ) : (
            <PatchView patch={patch} emptyMessage="This commit changed no files." />
          )}
        </div>
      )}
    </div>
  );
}

// --- Shared ------------------------------------------------------------------

function Loading() {
  return (
    <div className="grid flex-1 place-items-center">
      <Spinner className="size-4 text-[var(--text-faint)]" />
    </div>
  );
}

/**
 * Reading git can fail for ordinary reasons — the work was discarded, the folder
 * was moved — so this says what happened rather than showing an empty list and
 * letting it look like the run did nothing.
 */
function Problem({ message }: { message: string }) {
  return (
    <div className="p-3">
      <Badge tone="warn">
        <IconAlert className="size-3" />
        Cannot read the repository
      </Badge>
      <p className="mt-1.5 text-[11.5px] leading-snug text-[var(--text-muted)]">
        {message}
      </p>
    </div>
  );
}

function describe(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

/** "4m ago", "yesterday" — enough to place a commit without a full timestamp. */
function when(timestamp: string): string {
  const then = new Date(timestamp).getTime();
  if (Number.isNaN(then)) return "";

  const minutes = Math.round((Date.now() - then) / 60000);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes}m ago`;

  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours}h ago`;

  const days = Math.round(hours / 24);
  if (days === 1) return "yesterday";
  if (days < 30) return `${days}d ago`;
  return new Date(then).toLocaleDateString();
}
