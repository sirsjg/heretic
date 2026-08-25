/**
 * Rendering for a unified diff.
 *
 * The point of showing a diff inside Heretic is to decide whether to merge
 * without leaving for a terminal, so this optimises for reading rather than
 * editing: line numbers on both sides, changed lines tinted, and hunk headings
 * kept so a jump of four hundred lines is visibly a jump.
 */

import { useMemo, useState } from "react";
import type { DiffHunk, DiffLine, FileDiff } from "../lib/diff";
import { parseUnifiedDiff, splitPath } from "../lib/diff";
import { cx } from "./ui";
import { IconChevron } from "./icons";

/** How many lines to render before asking whether the rest is really wanted. */
const LINE_BUDGET = 800;

const LINE_TONE: Record<DiffLine["kind"], string | undefined> = {
  add: "var(--success-soft)",
  del: "var(--danger-soft)",
  context: undefined,
};

const SIGN: Record<DiffLine["kind"], string> = {
  add: "+",
  del: "−",
  context: " ",
};

/** A whole patch — one file or many — rendered file by file. */
export function PatchView({
  patch,
  emptyMessage = "No textual changes.",
}: {
  patch: string;
  emptyMessage?: string;
}) {
  const files = useMemo(() => parseUnifiedDiff(patch), [patch]);

  if (files.length === 0) {
    return <Note>{emptyMessage}</Note>;
  }

  return (
    <div className="flex flex-col gap-2">
      {files.map((file, index) => (
        <FileDiffView key={`${file.path}-${index}`} file={file} />
      ))}
    </div>
  );
}

/** One file's diff, with its own path header. */
export function FileDiffView({
  file,
  hideHeader,
}: {
  file: FileDiff;
  hideHeader?: boolean;
}) {
  const { directory, name } = splitPath(file.path);

  return (
    <div className="overflow-hidden rounded-lg border" style={{ background: "var(--surface)" }}>
      {!hideHeader && (
        <div
          className="flex items-center gap-2 border-b px-2.5 py-1.5"
          style={{ background: "var(--surface-2)" }}
        >
          <span className="min-w-0 truncate font-mono text-[11.5px]">
            <span className="text-[var(--text-faint)]">{directory}</span>
            <span>{name}</span>
          </span>
          {file.oldPath && file.oldPath !== file.path && (
            <span className="shrink-0 truncate font-mono text-[11px] text-[var(--text-faint)]">
              ← {file.oldPath}
            </span>
          )}
          <DiffStat additions={file.additions} deletions={file.deletions} />
        </div>
      )}
      <DiffBody file={file} />
    </div>
  );
}

function DiffBody({ file }: { file: FileDiff }) {
  const total = file.hunks.reduce((sum, hunk) => sum + hunk.lines.length, 0);
  const [expanded, setExpanded] = useState(total <= LINE_BUDGET);

  if (file.binary) return <Note>Binary file — nothing to show.</Note>;

  if (file.hunks.length === 0) {
    if (file.isRenamed) return <Note>Renamed, with no other change.</Note>;
    return <Note>No textual changes.</Note>;
  }

  const hunks = expanded ? file.hunks : trimTo(file.hunks, LINE_BUDGET);

  return (
    <>
      <div className="overflow-x-auto">
        <div className="min-w-max font-mono text-[11.5px] leading-[1.55]">
          {hunks.map((hunk, index) => (
            <Hunk key={index} hunk={hunk} first={index === 0} />
          ))}
        </div>
      </div>

      {!expanded && (
        <button
          onClick={() => setExpanded(true)}
          className="w-full border-t px-2.5 py-1.5 text-left text-[11.5px] text-[var(--text-muted)] hover:bg-[var(--surface-3)] hover:text-[var(--text)]"
        >
          Show the rest — {(total - LINE_BUDGET).toLocaleString()} more lines
        </button>
      )}

      {file.truncated && (
        <p
          className="border-t px-2.5 py-1.5 text-[11px]"
          style={{ color: "var(--warn)" }}
        >
          This diff was too large to send in full and has been cut short.
        </p>
      )}
    </>
  );
}

function Hunk({ hunk, first }: { hunk: DiffHunk; first: boolean }) {
  return (
    <>
      <div
        className={cx("flex items-center gap-2 px-2.5 py-1", !first && "border-t")}
        style={{ background: "var(--surface-2)", color: "var(--text-faint)" }}
      >
        <span className="text-[11px]">{hunkRange(hunk.header)}</span>
        {hunk.heading && (
          <span className="truncate text-[11px] opacity-80">{hunk.heading}</span>
        )}
      </div>

      {hunk.lines.map((line, index) => (
        <Line key={index} line={line} />
      ))}
    </>
  );
}

function Line({ line }: { line: DiffLine }) {
  return (
    <div className="flex" style={{ background: LINE_TONE[line.kind] }}>
      <Gutter value={line.oldNumber} />
      <Gutter value={line.newNumber} />
      <span
        aria-hidden
        className="w-4 shrink-0 select-none text-center"
        style={{
          color:
            line.kind === "add"
              ? "var(--success)"
              : line.kind === "del"
                ? "var(--danger)"
                : "var(--text-faint)",
        }}
      >
        {SIGN[line.kind]}
      </span>
      <span className="whitespace-pre pr-3">{line.text || " "}</span>
    </div>
  );
}

function Gutter({ value }: { value: number | null }) {
  return (
    <span
      className="w-11 shrink-0 select-none border-r px-1.5 text-right tabular-nums"
      style={{ color: "var(--text-faint)" }}
    >
      {value ?? ""}
    </span>
  );
}

/** "+12 −3", in the two colours used everywhere else for the same idea. */
export function DiffStat({
  additions,
  deletions,
  className,
}: {
  additions: number;
  deletions: number;
  className?: string;
}) {
  if (additions === 0 && deletions === 0) return null;

  return (
    <span className={cx("ml-auto shrink-0 font-mono text-[11px]", className)}>
      {additions > 0 && <span style={{ color: "var(--success)" }}>+{additions}</span>}
      {additions > 0 && deletions > 0 && " "}
      {deletions > 0 && <span style={{ color: "var(--danger)" }}>−{deletions}</span>}
    </span>
  );
}

/** A collapsible wrapper, used for a file in a list of many. */
export function Collapsible({
  open,
  onToggle,
  head,
  children,
}: {
  open: boolean;
  onToggle: () => void;
  head: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <div className="border-b last:border-b-0">
      <button
        onClick={onToggle}
        className="flex w-full items-center gap-2 px-2.5 py-1.5 text-left hover:bg-[var(--surface-3)]"
      >
        <IconChevron
          className="size-3 shrink-0 text-[var(--text-faint)] transition-transform"
          style={{ transform: open ? "rotate(90deg)" : undefined }}
        />
        {head}
      </button>
      {open && <div className="px-2.5 pb-2.5">{children}</div>}
    </div>
  );
}

function Note({ children }: { children: React.ReactNode }) {
  return (
    <p className="px-2.5 py-2 text-[11.5px] text-[var(--text-faint)]">{children}</p>
  );
}

/** Just the `-a,b +c,d` part of a hunk header. */
function hunkRange(header: string): string {
  const match = /@@+ (.+?) @@/.exec(header);
  return match ? match[1]!.trim() : header;
}

/** Keep whole hunks up to a line budget, so a huge file opens instantly. */
function trimTo(hunks: DiffHunk[], budget: number): DiffHunk[] {
  const kept: DiffHunk[] = [];
  let used = 0;

  for (const hunk of hunks) {
    if (used >= budget) break;
    kept.push(hunk);
    used += hunk.lines.length;
  }

  return kept.length > 0 ? kept : hunks.slice(0, 1);
}
