/**
 * A unified-diff parser.
 *
 * The engine hands the interface exactly what `git diff` printed, and this turns
 * it into something renderable: files, hunks, and lines that know which side of
 * the change they are on and what line number they carry.
 *
 * It is deliberately tolerant. A diff is a text format produced by a program
 * that has more options than anyone remembers, and a run's changes are worth
 * showing even when one file in them is a rename with no hunks, a binary blob,
 * or a mode change on its own.
 */

export type DiffLineKind = "add" | "del" | "context";

export interface DiffLine {
  kind: DiffLineKind;
  text: string;
  /** Line number on the left (before) side, when the line exists there. */
  oldNumber: number | null;
  /** Line number on the right (after) side, when the line exists there. */
  newNumber: number | null;
}

export interface DiffHunk {
  /** The `@@ … @@` line, including the section heading git appends. */
  header: string;
  /** Whatever git wrote after the closing `@@` — usually the enclosing function. */
  heading: string;
  lines: DiffLine[];
}

export interface FileDiff {
  path: string;
  /** Where a renamed file came from. */
  oldPath: string | null;
  hunks: DiffHunk[];
  additions: number;
  deletions: number;
  binary: boolean;
  isNew: boolean;
  isDeleted: boolean;
  isRenamed: boolean;
  /** Set when the engine cut a very large diff short. */
  truncated: boolean;
}

const HUNK = /^@@+ (.+?) @@(.*)$/;
const RANGE = /^-(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))?$/;

/** A file entry with nothing filled in yet. */
function emptyFile(path: string): FileDiff {
  return {
    path,
    oldPath: null,
    hunks: [],
    additions: 0,
    deletions: 0,
    binary: false,
    isNew: false,
    isDeleted: false,
    isRenamed: false,
    truncated: false,
  };
}

/** Split a patch into files. Handles `git diff`, `git show` and `--no-index`. */
export function parseUnifiedDiff(patch: string): FileDiff[] {
  const files: FileDiff[] = [];
  let file: FileDiff | null = null;
  let hunk: DiffHunk | null = null;
  let oldNumber = 0;
  let newNumber = 0;

  const lines = patch.split("\n");
  // A patch ends with a newline, so splitting leaves one empty element behind.
  // Kept, it would render as a blank context line at the foot of every file —
  // and a blank line genuinely inside a hunk is a space, not nothing.
  if (lines.at(-1) === "") lines.pop();

  for (const line of lines) {
    if (line.startsWith("diff --git ")) {
      file = emptyFile(readGitHeader(line));
      hunk = null;
      files.push(file);
      continue;
    }

    // The engine says so in plain words when it cut a diff short.
    if (line.startsWith("[diff truncated")) {
      if (file) file.truncated = true;
      continue;
    }

    if (!file) {
      // `--no-index` output, and some tools, start straight at `---`.
      if (line.startsWith("--- ")) {
        file = emptyFile(stripPrefix(line.slice(4)) ?? "");
        hunk = null;
        files.push(file);
      }
      continue;
    }

    if (line.startsWith("new file mode")) {
      file.isNew = true;
      continue;
    }
    if (line.startsWith("deleted file mode")) {
      file.isDeleted = true;
      continue;
    }
    if (line.startsWith("rename from ")) {
      file.isRenamed = true;
      file.oldPath = line.slice("rename from ".length).trim();
      continue;
    }
    if (line.startsWith("rename to ")) {
      file.isRenamed = true;
      file.path = line.slice("rename to ".length).trim();
      continue;
    }
    if (line.startsWith("Binary files") || line.startsWith("GIT binary patch")) {
      file.binary = true;
      continue;
    }

    if (line.startsWith("--- ")) {
      const path = stripPrefix(line.slice(4));
      if (path === null) file.isNew = true;
      else if (!file.oldPath) file.oldPath = path;
      continue;
    }

    if (line.startsWith("+++ ")) {
      const path = stripPrefix(line.slice(4));
      if (path === null) file.isDeleted = true;
      else file.path = path;
      continue;
    }

    const match = HUNK.exec(line);
    if (match) {
      const range = RANGE.exec(match[1]!.trim());
      oldNumber = range ? Number(range[1]) : 0;
      newNumber = range ? Number(range[3]) : 0;
      hunk = { header: line, heading: match[2]!.trim(), lines: [] };
      file.hunks.push(hunk);
      continue;
    }

    if (!hunk) continue;

    // "\ No newline at end of file" annotates the line above it rather than
    // being a line of its own.
    if (line.startsWith("\\")) continue;

    if (line.startsWith("+")) {
      hunk.lines.push({
        kind: "add",
        text: line.slice(1),
        oldNumber: null,
        newNumber: newNumber++,
      });
      file.additions += 1;
    } else if (line.startsWith("-")) {
      hunk.lines.push({
        kind: "del",
        text: line.slice(1),
        oldNumber: oldNumber++,
        newNumber: null,
      });
      file.deletions += 1;
    } else if (line.startsWith(" ") || line === "") {
      hunk.lines.push({
        kind: "context",
        text: line.slice(1),
        oldNumber: oldNumber++,
        newNumber: newNumber++,
      });
    }
  }

  return files;
}

/** How many lines a patch touches, without keeping the parse around. */
export function countDiff(patch: string): { additions: number; deletions: number } {
  let additions = 0;
  let deletions = 0;

  for (const line of patch.split("\n")) {
    if (line.startsWith("+") && !line.startsWith("+++")) additions += 1;
    else if (line.startsWith("-") && !line.startsWith("---")) deletions += 1;
  }

  return { additions, deletions };
}

/**
 * The destination path from `diff --git a/one b/two`, where either side may be
 * quoted and either may contain spaces. Splitting on the last " b/" is the
 * pragmatic reading git's own tools use, and the `+++` line that follows
 * corrects it a moment later anyway.
 */
function readGitHeader(line: string): string {
  const rest = line.slice("diff --git ".length);
  const split = rest.lastIndexOf(" b/");
  if (split < 0) return unquote(rest);
  return unquote(rest.slice(split + 1)).replace(/^b\//, "");
}

/** Drop git's `a/`, `b/` prefixes. `/dev/null` means the file is not there. */
function stripPrefix(value: string): string | null {
  const path = unquote(value.replace(/\t.*$/, "").trim());
  if (path === "/dev/null") return null;
  return path.replace(/^[ab]\//, "");
}

/** git quotes paths that contain unusual characters. */
function unquote(value: string): string {
  const trimmed = value.trim();
  if (!trimmed.startsWith('"') || !trimmed.endsWith('"')) return trimmed;
  try {
    return JSON.parse(trimmed) as string;
  } catch {
    return trimmed.slice(1, -1);
  }
}

/** A path split for display: the directory dimmed, the file name not. */
export function splitPath(path: string): { directory: string; name: string } {
  const index = path.lastIndexOf("/");
  if (index < 0) return { directory: "", name: path };
  return { directory: path.slice(0, index + 1), name: path.slice(index + 1) };
}
