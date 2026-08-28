//! Git worktree management.
//!
//! When several agents work one project at once they must not share a checkout —
//! two agents editing the same tree corrupt each other's work. Each task run gets
//! its own `git worktree` on its own branch, created off the project's base
//! branch and removed once the work has been integrated or discarded.
//!
//! Worktrees are created outside the user's repository (under Heretic's data
//! directory) so the agent never sees other runs' checkouts while it works.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("{path} is not a git repository")]
    NotARepository { path: String },

    #[error("git {operation} failed: {message}")]
    Command { operation: String, message: String },

    #[error("could not run git: {0}")]
    Spawn(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, GitError>;

/// A checkout created for one task run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    /// Where the agent works.
    pub path: PathBuf,
    /// The branch the work lands on.
    pub branch: String,
    /// The branch it was forked from.
    pub base_branch: String,
}

/// What changed in a worktree.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ChangeSummary {
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
    pub files: Vec<String>,
}

impl ChangeSummary {
    pub fn is_empty(&self) -> bool {
        self.files_changed == 0
    }
}

/// Run a git command in `dir` and return its stdout.
async fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .await?;

    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(GitError::Command {
            operation: args.first().copied().unwrap_or("git").to_string(),
            message: if message.is_empty() {
                String::from_utf8_lossy(&output.stdout).trim().to_string()
            } else {
                message
            },
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Whether `path` is inside a git working tree.
pub async fn is_repository(path: &Path) -> bool {
    git(path, &["rev-parse", "--is-inside-work-tree"])
        .await
        .map(|out| out == "true")
        .unwrap_or(false)
}

/// The repository's current branch, or `HEAD` when detached.
pub async fn current_branch(repo: &Path) -> Result<String> {
    ensure_repository(repo).await?;
    git(repo, &["rev-parse", "--abbrev-ref", "HEAD"]).await
}

/// True when the checkout has uncommitted changes.
pub async fn is_dirty(repo: &Path) -> Result<bool> {
    ensure_repository(repo).await?;
    Ok(!git(repo, &["status", "--porcelain"]).await?.is_empty())
}

async fn ensure_repository(path: &Path) -> Result<()> {
    if is_repository(path).await {
        Ok(())
    } else {
        Err(GitError::NotARepository {
            path: path.display().to_string(),
        })
    }
}

/// Turn a task title into something usable in a branch name.
pub fn slugify(value: &str, max_len: usize) -> String {
    let mut slug = String::new();
    let mut last_dash = true;

    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && slug.len() < max_len {
            slug.push('-');
            last_dash = true;
        }
        if slug.len() >= max_len {
            break;
        }
    }

    slug.trim_matches('-').to_string()
}

/// The branch name for a task run. Namespaced so it is obvious in `git branch`
/// which branches Heretic created.
pub fn branch_name(task_id: &str, task_title: &str) -> String {
    let slug = slugify(task_title, 40);
    if slug.is_empty() {
        format!("heretic/{task_id}")
    } else {
        format!("heretic/{task_id}-{slug}")
    }
}

/// Create a worktree for a task, forked from `base_branch` (or the repository's
/// current branch when `None`).
///
/// `root` is the directory worktrees are created under — kept outside the
/// repository so agents never stumble into each other's checkouts.
pub async fn create_worktree(
    repo: &Path,
    root: &Path,
    task_id: &str,
    task_title: &str,
    base_branch: Option<&str>,
) -> Result<Worktree> {
    ensure_repository(repo).await?;

    let base = match base_branch {
        Some(branch) if !branch.trim().is_empty() => branch.trim().to_string(),
        _ => current_branch(repo).await?,
    };

    let branch = branch_name(task_id, task_title);
    let path = root.join(task_id);

    // A leftover worktree from an interrupted run would block creation.
    if path.exists() {
        let _ = remove_worktree(repo, &path).await;
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let path_str = path.to_string_lossy().to_string();
    let existing_branch = git(repo, &["rev-parse", "--verify", &branch]).await.is_ok();

    let args: Vec<&str> = if existing_branch {
        // Resuming a task that already has a branch: check it out as-is.
        vec!["worktree", "add", &path_str, &branch]
    } else {
        vec!["worktree", "add", "-b", &branch, &path_str, &base]
    };
    git(repo, &args).await?;

    Ok(Worktree {
        path,
        branch,
        base_branch: base,
    })
}

/// Remove a worktree directory and prune git's record of it. The branch is left
/// alone so the work is never lost by accident.
pub async fn remove_worktree(repo: &Path, worktree_path: &Path) -> Result<()> {
    let path_str = worktree_path.to_string_lossy().to_string();
    // --force because the agent will have left files behind.
    let removed = git(repo, &["worktree", "remove", "--force", &path_str]).await;

    if removed.is_err() && worktree_path.exists() {
        std::fs::remove_dir_all(worktree_path)?;
    }
    git(repo, &["worktree", "prune"]).await?;
    Ok(())
}

/// Summarise what an agent changed, counting both committed and uncommitted work.
pub async fn summarise_changes(worktree: &Path, base_branch: &str) -> Result<ChangeSummary> {
    ensure_repository(worktree).await?;

    // Compare against the fork point so the summary covers commits the agent made
    // as well as anything still uncommitted.
    let merge_base = git(worktree, &["merge-base", "HEAD", base_branch])
        .await
        .unwrap_or_else(|_| "HEAD".to_string());

    let numstat = git(worktree, &["diff", "--numstat", &merge_base]).await?;
    let untracked = git(worktree, &["ls-files", "--others", "--exclude-standard"]).await?;

    let mut summary = ChangeSummary::default();

    for line in numstat.lines().filter(|l| !l.trim().is_empty()) {
        let mut parts = line.split('\t');
        let added = parts.next().unwrap_or("0");
        let removed = parts.next().unwrap_or("0");
        let file = parts.next().unwrap_or("").trim();

        // Binary files report "-" instead of a count.
        summary.insertions += added.parse::<usize>().unwrap_or(0);
        summary.deletions += removed.parse::<usize>().unwrap_or(0);
        if !file.is_empty() {
            summary.files.push(file.to_string());
        }
    }

    for file in untracked.lines().filter(|l| !l.trim().is_empty()) {
        summary.files.push(file.trim().to_string());
    }

    summary.files.sort();
    summary.files.dedup();
    summary.files_changed = summary.files.len();
    Ok(summary)
}

/// The full diff of a run, for handing to a reviewer agent.
///
/// `max_bytes` truncates very large diffs — a reviewer that never sees the end of
/// a diff is worse than one told plainly that it was cut short.
pub async fn full_diff(worktree: &Path, base_branch: &str, max_bytes: usize) -> Result<String> {
    ensure_repository(worktree).await?;

    let merge_base = git(worktree, &["merge-base", "HEAD", base_branch])
        .await
        .unwrap_or_else(|_| "HEAD".to_string());

    // Stage untracked files so they appear in the diff, without committing.
    let _ = git(worktree, &["add", "--intent-to-add", "."]).await;
    let diff = git(worktree, &["diff", &merge_base]).await?;

    if diff.len() <= max_bytes {
        return Ok(diff);
    }

    let kept: String = diff.chars().take(max_bytes).collect();
    Ok(format!(
        "{kept}\n\n[diff truncated at {max_bytes} bytes — {} bytes omitted]",
        diff.len().saturating_sub(max_bytes)
    ))
}

/// Commit everything in a worktree. Returns `false` when there was nothing to commit.
pub async fn commit_all(worktree: &Path, message: &str) -> Result<bool> {
    ensure_repository(worktree).await?;
    git(worktree, &["add", "-A"]).await?;

    if git(worktree, &["diff", "--cached", "--quiet"])
        .await
        .is_ok()
    {
        return Ok(false); // exit 0 from --quiet means no staged changes
    }

    git(worktree, &["commit", "-m", message]).await?;
    Ok(true)
}

/// Merge a run's branch back into the base branch, in the main checkout.
///
/// Uses `--no-ff` so every autonomous run stays visible as a distinct merge in
/// history rather than being flattened into the base branch.
pub async fn merge_branch(repo: &Path, branch: &str, base_branch: &str) -> Result<()> {
    ensure_repository(repo).await?;

    if is_dirty(repo).await? {
        return Err(GitError::Command {
            operation: "merge".into(),
            message: "the main checkout has uncommitted changes; commit or stash them first".into(),
        });
    }

    git(repo, &["checkout", base_branch]).await?;
    let message = format!("Merge {branch}");
    git(repo, &["merge", "--no-ff", "-m", &message, branch]).await?;
    Ok(())
}

/// Delete a run's branch. Refuses while a worktree still has it checked out, so
/// remove the worktree first.
///
/// Uses `-D`: a discarded run's branch is deliberately unmerged, and `-d` would
/// refuse precisely when we mean it.
pub async fn delete_branch(repo: &Path, branch: &str) -> Result<()> {
    ensure_repository(repo).await?;
    git(repo, &["branch", "-D", branch]).await?;
    Ok(())
}

/// Whether a branch has already been merged into `base`.
pub async fn is_merged(repo: &Path, branch: &str, base: &str) -> Result<bool> {
    ensure_repository(repo).await?;
    // merge-base --is-ancestor exits 0 when branch is contained in base.
    Ok(git(repo, &["merge-base", "--is-ancestor", branch, base])
        .await
        .is_ok())
}

/// Worktrees Heretic created that git still knows about, used to clean up
/// after a crash.
pub async fn list_heretic_worktrees(repo: &Path) -> Result<Vec<PathBuf>> {
    let output = git(repo, &["worktree", "list", "--porcelain"]).await?;
    let mut paths = Vec::new();
    let mut current: Option<PathBuf> = None;

    for line in output.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            current = Some(PathBuf::from(path.trim()));
        } else if let Some(branch) = line.strip_prefix("branch ") {
            if branch.contains("heretic/") {
                if let Some(path) = current.take() {
                    paths.push(path);
                }
            }
        }
    }

    Ok(paths)
}

// --- Reading a run's work ----------------------------------------------------
//
// Everything below is read-only. The interface uses it to show what a run
// actually did — the files it touched, the diff of each one, and the commits it
// left on its branch — without anyone having to leave the app for a terminal.

/// What happened to one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    /// Written but never added to git — it exists only in the working tree.
    Untracked,
}

impl ChangeStatus {
    fn from_letter(letter: char) -> Self {
        match letter {
            'A' => ChangeStatus::Added,
            'D' => ChangeStatus::Deleted,
            'R' => ChangeStatus::Renamed,
            _ => ChangeStatus::Modified,
        }
    }
}

/// One file in a run's diff, with the numbers the interface shows beside it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileChange {
    pub path: String,
    /// Where a renamed file came from.
    #[serde(default)]
    pub old_path: Option<String>,
    pub status: ChangeStatus,
    pub insertions: usize,
    pub deletions: usize,
    /// Binary files have no line counts and no readable diff.
    pub binary: bool,
}

/// A commit on a run's branch.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Commit {
    pub sha: String,
    pub short_sha: String,
    pub author: String,
    pub email: String,
    /// RFC 3339, as git's `%aI` writes it.
    pub authored_at: String,
    pub subject: String,
    pub body: String,
    pub files_changed: usize,
    pub insertions: usize,
    pub deletions: usize,
}

/// Which checkout to read, and what counts as "the run's work" inside it.
///
/// A run is readable from two places over its life: its own worktree while that
/// exists, and — once the worktree has been removed by a merge — the branch it
/// left behind in the project's own checkout. Both are described here so the
/// callers below do not care which one they were given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffScope {
    pub dir: PathBuf,
    /// The branch the work forked from.
    pub base_branch: String,
    /// The tip to compare against. `None` reads the working tree, so
    /// uncommitted and untracked work is included.
    pub head: Option<String>,
}

impl DiffScope {
    /// Read a live checkout, including work the agent has not committed.
    pub fn working_tree(dir: impl Into<PathBuf>, base_branch: impl Into<String>) -> Self {
        Self {
            dir: dir.into(),
            base_branch: base_branch.into(),
            head: None,
        }
    }

    /// Read a branch from a checkout that is sitting on something else.
    pub fn branch(
        dir: impl Into<PathBuf>,
        base_branch: impl Into<String>,
        branch: impl Into<String>,
    ) -> Self {
        Self {
            dir: dir.into(),
            base_branch: base_branch.into(),
            head: Some(branch.into()),
        }
    }

    fn tip(&self) -> &str {
        self.head.as_deref().unwrap_or("HEAD")
    }

    /// Where this work forked from the base branch.
    ///
    /// Normally that is the merge base. Once the work has been merged the merge
    /// base becomes the branch's own tip — the base contains everything — and a
    /// diff against it is empty, which would blank out the run's changes and
    /// history the moment it landed. So a merged branch is traced back through
    /// the merge commit that brought it in instead.
    async fn fork_point(&self) -> String {
        let tip = self.tip();
        let merge_base = git(&self.dir, &["merge-base", tip, &self.base_branch])
            .await
            .unwrap_or_else(|_| tip.to_string());

        let Ok(tip_sha) = git(&self.dir, &["rev-parse", tip]).await else {
            return merge_base;
        };
        if merge_base != tip_sha {
            return merge_base;
        }

        self.fork_point_before_merge(&tip_sha)
            .await
            .unwrap_or(merge_base)
    }

    /// The merge base as it was before this branch landed, found from the merge
    /// commit on the base branch whose second parent is this branch's tip.
    ///
    /// Heretic merges with `--no-ff` precisely so every run stays visible in
    /// history, which is what makes this findable.
    async fn fork_point_before_merge(&self, tip_sha: &str) -> Option<String> {
        let merges = git_lenient(
            &self.dir,
            &[
                "rev-list",
                "--parents",
                "--merges",
                "--first-parent",
                "-n",
                "500",
                &self.base_branch,
            ],
        )
        .await
        .ok()?;

        for line in merges.lines() {
            // "<merge> <first parent> <second parent>"
            let mut ids = line.split_whitespace().skip(1);
            let first = ids.next()?;
            if ids.next() != Some(tip_sha) {
                continue;
            }
            return git(&self.dir, &["merge-base", first, tip_sha]).await.ok();
        }

        None
    }
}

/// Run a git command and return its stdout untrimmed, even when git reports a
/// non-zero exit.
///
/// `git diff` exits 1 whenever it finds a difference, which is the normal case
/// here, and `--numstat -z` output must not be trimmed because its records are
/// NUL-terminated.
async fn git_lenient(dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .await?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Whether a revision resolves in this checkout.
pub async fn has_revision(dir: &Path, revision: &str) -> bool {
    git(dir, &["rev-parse", "--verify", "--quiet", revision])
        .await
        .is_ok()
}

/// A commit id we are willing to hand to git, so nothing from the interface can
/// arrive as an option or a pathspec.
pub fn is_commit_id(value: &str) -> bool {
    let length = value.len();
    (4..=40).contains(&length) && value.chars().all(|c| c.is_ascii_hexdigit())
}

/// Every file the run touched, with its line counts.
pub async fn changed_files(scope: &DiffScope) -> Result<Vec<FileChange>> {
    ensure_repository(&scope.dir).await?;
    let fork_point = scope.fork_point().await;

    let mut range: Vec<&str> = vec![&fork_point];
    if let Some(head) = scope.head.as_deref() {
        range.push(head);
    }

    let mut args = vec!["diff", "--numstat", "-z", "--find-renames"];
    args.extend_from_slice(&range);
    let numstat = git_lenient(&scope.dir, &args).await?;

    let mut args = vec!["diff", "--name-status", "-z", "--find-renames"];
    args.extend_from_slice(&range);
    let statuses = parse_name_status_z(&git_lenient(&scope.dir, &args).await?);

    let mut changes: Vec<FileChange> = parse_numstat_z(&numstat)
        .into_iter()
        .map(|record| FileChange {
            status: statuses
                .get(&record.path)
                .copied()
                .unwrap_or(ChangeStatus::Modified),
            old_path: record.old_path,
            path: record.path,
            insertions: record.insertions,
            deletions: record.deletions,
            binary: record.binary,
        })
        .collect();

    // Untracked files are part of what an agent did, and git's diff does not
    // know about them. Only a live checkout can have any.
    if scope.head.is_none() {
        let untracked = git(&scope.dir, &["ls-files", "--others", "--exclude-standard"]).await?;

        for path in untracked.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let (lines, binary) = measure_untracked(&scope.dir.join(path));
            changes.push(FileChange {
                path: path.to_string(),
                old_path: None,
                status: ChangeStatus::Untracked,
                insertions: lines,
                deletions: 0,
                binary,
            });
        }
    }

    changes.sort_by(|a, b| a.path.cmp(&b.path));
    changes.dedup_by(|a, b| a.path == b.path);
    Ok(changes)
}

/// The diff of a single file, as a unified patch the interface can render.
pub async fn file_diff(scope: &DiffScope, path: &str, max_bytes: usize) -> Result<String> {
    ensure_repository(&scope.dir).await?;
    let fork_point = scope.fork_point().await;

    let mut args = vec!["diff", "--find-renames", &fork_point];
    if let Some(head) = scope.head.as_deref() {
        args.push(head);
    }
    args.push("--");
    args.push(path);

    let diff = git_lenient(&scope.dir, &args).await?;
    if !diff.trim().is_empty() {
        return Ok(truncate_diff(diff, max_bytes));
    }

    // Nothing from `git diff` means the file is untracked: compare it against
    // nothing, which produces the same patch format without touching the index.
    let absolute = scope.dir.join(path);
    if scope.head.is_none() && absolute.exists() {
        let absolute = absolute.to_string_lossy().to_string();
        let diff = git_lenient(
            &scope.dir,
            &["diff", "--no-index", "--", "/dev/null", &absolute],
        )
        .await?;
        // `--no-index` names the file by the path it was given; put it back to
        // the repository-relative one so the interface shows something sane.
        let diff = diff.replace(&absolute, path);
        return Ok(truncate_diff(diff, max_bytes));
    }

    Ok(diff)
}

/// The commits a run put on its branch, newest first.
pub async fn commits(scope: &DiffScope, limit: usize) -> Result<Vec<Commit>> {
    ensure_repository(&scope.dir).await?;
    let fork_point = scope.fork_point().await;
    let range = format!("{fork_point}..{}", scope.tip());
    let limit = format!("-{limit}");

    // 0x1e starts a record and 0x1f separates its fields, so a commit message
    // containing newlines — which most do — cannot be mistaken for the next one.
    let format = "--format=%x1e%H%x1f%h%x1f%an%x1f%ae%x1f%aI%x1f%s%x1f%b%x1f";
    let raw = git_lenient(
        &scope.dir,
        &["log", &limit, "--numstat", "--no-color", format, &range],
    )
    .await?;

    Ok(raw
        .split('\u{1e}')
        .filter(|record| !record.trim().is_empty())
        .filter_map(parse_commit_record)
        .collect())
}

/// The patch a single commit introduced.
pub async fn commit_diff(dir: &Path, sha: &str, max_bytes: usize) -> Result<String> {
    ensure_repository(dir).await?;

    if !is_commit_id(sha) {
        return Err(GitError::Command {
            operation: "show".into(),
            message: format!("{sha} is not a commit id"),
        });
    }

    // An empty `--format` leaves just the patch, which is all the interface
    // renders — it already has the commit's own details.
    let diff = git_lenient(
        dir,
        &[
            "show",
            "--format=",
            "--patch",
            "--find-renames",
            "--no-color",
            sha,
        ],
    )
    .await?;

    Ok(truncate_diff(diff, max_bytes))
}

fn truncate_diff(diff: String, max_bytes: usize) -> String {
    if diff.len() <= max_bytes {
        return diff;
    }
    let kept: String = diff.chars().take(max_bytes).collect();
    let omitted = diff.len().saturating_sub(kept.len());
    format!("{kept}\n\n[diff truncated — {omitted} bytes omitted]\n")
}

struct NumstatRecord {
    insertions: usize,
    deletions: usize,
    binary: bool,
    path: String,
    old_path: Option<String>,
}

/// Parse `git diff --numstat -z`.
///
/// Each record is `added \t removed \t path` terminated by NUL, except a rename,
/// which leaves the path empty and follows with two more NUL-terminated fields:
/// where the file came from and where it went.
fn parse_numstat_z(raw: &str) -> Vec<NumstatRecord> {
    let mut fields = raw.split('\0');
    let mut records = Vec::new();

    while let Some(entry) = fields.next() {
        if entry.trim().is_empty() {
            continue;
        }

        let mut parts = entry.splitn(3, '\t');
        let added = parts.next().unwrap_or("");
        let removed = parts.next().unwrap_or("");
        let path = parts.next().unwrap_or("").trim_start_matches('\n');

        let (old_path, path) = if path.is_empty() {
            let old = fields.next().unwrap_or("").to_string();
            let new = fields.next().unwrap_or("").to_string();
            (Some(old), new)
        } else {
            (None, path.to_string())
        };

        if path.is_empty() {
            continue;
        }

        records.push(NumstatRecord {
            // Binary files report "-" where a count would be.
            insertions: added.parse().unwrap_or(0),
            deletions: removed.parse().unwrap_or(0),
            binary: added == "-" || removed == "-",
            path,
            old_path,
        });
    }

    records
}

/// Parse `git diff --name-status -z` into a status per destination path.
fn parse_name_status_z(raw: &str) -> std::collections::HashMap<String, ChangeStatus> {
    let mut fields = raw.split('\0').filter(|f| !f.is_empty());
    let mut statuses = std::collections::HashMap::new();

    while let Some(status) = fields.next() {
        let Some(letter) = status.trim().chars().next() else {
            continue;
        };

        // A rename or a copy carries both the old path and the new one.
        let path = if matches!(letter, 'R' | 'C') {
            let _from = fields.next();
            fields.next()
        } else {
            fields.next()
        };

        if let Some(path) = path {
            statuses.insert(path.to_string(), ChangeStatus::from_letter(letter));
        }
    }

    statuses
}

fn parse_commit_record(record: &str) -> Option<Commit> {
    let mut parts = record.splitn(8, '\u{1f}');
    let sha = parts.next()?.trim().to_string();
    let short_sha = parts.next()?.to_string();
    let author = parts.next()?.to_string();
    let email = parts.next()?.to_string();
    let authored_at = parts.next()?.to_string();
    let subject = parts.next()?.to_string();
    let body = parts.next().unwrap_or("").trim().to_string();
    let numstat = parts.next().unwrap_or("");

    if sha.is_empty() {
        return None;
    }

    let mut commit = Commit {
        sha,
        short_sha,
        author,
        email,
        authored_at,
        subject,
        body,
        files_changed: 0,
        insertions: 0,
        deletions: 0,
    };

    for line in numstat.lines().filter(|line| !line.trim().is_empty()) {
        let mut columns = line.split('\t');
        let added = columns.next().unwrap_or("0");
        let removed = columns.next().unwrap_or("0");
        if columns.next().is_none() {
            continue;
        }
        commit.files_changed += 1;
        commit.insertions += added.parse::<usize>().unwrap_or(0);
        commit.deletions += removed.parse::<usize>().unwrap_or(0);
    }

    Some(commit)
}

/// How big an untracked file to read when counting its lines. Past this it is
/// almost certainly not something a person is going to review line by line.
const UNTRACKED_SCAN_LIMIT: usize = 2 * 1024 * 1024;

/// Line count and binary-ness of a file git has never seen.
fn measure_untracked(path: &Path) -> (usize, bool) {
    let Ok(bytes) = std::fs::read(path) else {
        return (0, false);
    };

    if bytes.len() > UNTRACKED_SCAN_LIMIT {
        return (0, true);
    }
    // git's own heuristic: a NUL byte near the start means binary.
    if bytes.iter().take(8000).any(|byte| *byte == 0) {
        return (0, true);
    }

    let lines = bytes.iter().filter(|byte| **byte == b'\n').count();
    // A last line with no newline still counts.
    let trailing = usize::from(!bytes.is_empty() && !bytes.ends_with(b"\n"));
    (lines + trailing, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a throwaway repository with one commit.
    async fn fixture_repo(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("heretic-git-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        git(&root, &["init", "-b", "main"]).await.unwrap();
        git(&root, &["config", "user.email", "test@example.com"])
            .await
            .unwrap();
        git(&root, &["config", "user.name", "Test"]).await.unwrap();
        std::fs::write(root.join("README.md"), "hello\n").unwrap();
        git(&root, &["add", "-A"]).await.unwrap();
        git(&root, &["commit", "-m", "initial"]).await.unwrap();

        root
    }

    #[test]
    fn slugs_are_branch_safe() {
        assert_eq!(
            slugify("Add OAuth 2.0 support!", 40),
            "add-oauth-2-0-support"
        );
        assert_eq!(slugify("   ", 40), "");
        assert_eq!(slugify("a".repeat(100).as_str(), 10).len(), 10);
    }

    #[test]
    fn branch_names_are_namespaced() {
        assert_eq!(
            branch_name("abc123", "Fix the login bug"),
            "heretic/abc123-fix-the-login-bug"
        );
        // A title with nothing usable still produces a valid branch.
        assert_eq!(branch_name("abc123", "!!!"), "heretic/abc123");
    }

    #[tokio::test]
    async fn a_worktree_is_created_on_its_own_branch() {
        let repo = fixture_repo("create").await;
        let root = repo.join("..").join("heretic-worktrees-create");

        let worktree = create_worktree(&repo, &root, "task1", "Add a feature", None)
            .await
            .unwrap();

        assert!(worktree.path.exists());
        assert_eq!(worktree.branch, "heretic/task1-add-a-feature");
        assert_eq!(worktree.base_branch, "main");
        assert_eq!(
            current_branch(&worktree.path).await.unwrap(),
            "heretic/task1-add-a-feature"
        );

        remove_worktree(&repo, &worktree.path).await.unwrap();
        assert!(!worktree.path.exists());
        let _ = std::fs::remove_dir_all(&repo);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn changes_are_summarised_including_untracked_files() {
        let repo = fixture_repo("summary").await;
        let root = repo.join("..").join("heretic-worktrees-summary");
        let worktree = create_worktree(&repo, &root, "task2", "Work", None)
            .await
            .unwrap();

        std::fs::write(worktree.path.join("README.md"), "hello\nworld\n").unwrap();
        std::fs::write(worktree.path.join("new.txt"), "brand new\n").unwrap();

        let summary = summarise_changes(&worktree.path, "main").await.unwrap();
        assert!(summary.files.contains(&"README.md".to_string()));
        assert!(summary.files.contains(&"new.txt".to_string()));
        assert_eq!(summary.files_changed, 2);
        assert!(summary.insertions >= 1);

        remove_worktree(&repo, &worktree.path).await.unwrap();
        let _ = std::fs::remove_dir_all(&repo);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn an_untouched_worktree_reports_no_changes() {
        let repo = fixture_repo("clean").await;
        let root = repo.join("..").join("heretic-worktrees-clean");
        let worktree = create_worktree(&repo, &root, "task3", "Nothing", None)
            .await
            .unwrap();

        let summary = summarise_changes(&worktree.path, "main").await.unwrap();
        assert!(summary.is_empty());
        assert!(!commit_all(&worktree.path, "empty").await.unwrap());

        remove_worktree(&repo, &worktree.path).await.unwrap();
        let _ = std::fs::remove_dir_all(&repo);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn work_can_be_committed_and_merged_back() {
        let repo = fixture_repo("merge").await;
        let root = repo.join("..").join("heretic-worktrees-merge");
        let worktree = create_worktree(&repo, &root, "task4", "Add file", None)
            .await
            .unwrap();

        std::fs::write(worktree.path.join("feature.txt"), "shipped\n").unwrap();
        assert!(commit_all(&worktree.path, "Add feature").await.unwrap());

        merge_branch(&repo, &worktree.branch, "main").await.unwrap();
        assert!(repo.join("feature.txt").exists());

        remove_worktree(&repo, &worktree.path).await.unwrap();
        let _ = std::fs::remove_dir_all(&repo);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn a_dirty_main_checkout_refuses_the_merge() {
        let repo = fixture_repo("dirty").await;
        let root = repo.join("..").join("heretic-worktrees-dirty");
        let worktree = create_worktree(&repo, &root, "task5", "Add file", None)
            .await
            .unwrap();
        std::fs::write(worktree.path.join("feature.txt"), "shipped\n").unwrap();
        commit_all(&worktree.path, "Add feature").await.unwrap();

        // Uncommitted work in the user's checkout must never be clobbered.
        std::fs::write(repo.join("README.md"), "local edits\n").unwrap();
        let result = merge_branch(&repo, &worktree.branch, "main").await;
        assert!(result.is_err());

        remove_worktree(&repo, &worktree.path).await.unwrap();
        let _ = std::fs::remove_dir_all(&repo);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn discarded_work_removes_both_the_worktree_and_the_branch() {
        let repo = fixture_repo("discard").await;
        let root = repo.join("..").join("heretic-worktrees-discard");
        let worktree = create_worktree(&repo, &root, "task7", "Throwaway", None)
            .await
            .unwrap();

        std::fs::write(worktree.path.join("scratch.txt"), "junk\n").unwrap();
        commit_all(&worktree.path, "Some work").await.unwrap();

        // The worktree holds the branch checked out, so it goes first.
        remove_worktree(&repo, &worktree.path).await.unwrap();
        delete_branch(&repo, &worktree.branch).await.unwrap();

        let branches = git(&repo, &["branch", "--list", &worktree.branch])
            .await
            .unwrap();
        assert!(branches.trim().is_empty(), "branch survived: {branches}");
        assert!(!worktree.path.exists());
        // The user's own work is untouched.
        assert!(!repo.join("scratch.txt").exists());

        let _ = std::fs::remove_dir_all(&repo);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn unmerged_work_can_still_be_discarded() {
        // `git branch -d` refuses an unmerged branch, which is exactly the case
        // discarding exists for.
        let repo = fixture_repo("unmerged").await;
        let root = repo.join("..").join("heretic-worktrees-unmerged");
        let worktree = create_worktree(&repo, &root, "task8", "Rejected", None)
            .await
            .unwrap();

        std::fs::write(worktree.path.join("bad.txt"), "no\n").unwrap();
        commit_all(&worktree.path, "Work to throw away")
            .await
            .unwrap();
        assert!(!is_merged(&repo, &worktree.branch, "main").await.unwrap());

        remove_worktree(&repo, &worktree.path).await.unwrap();
        delete_branch(&repo, &worktree.branch).await.unwrap();

        let _ = std::fs::remove_dir_all(&repo);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn merged_work_is_recognised_as_landed() {
        let repo = fixture_repo("landed").await;
        let root = repo.join("..").join("heretic-worktrees-landed");
        let worktree = create_worktree(&repo, &root, "task9", "Landing", None)
            .await
            .unwrap();

        std::fs::write(worktree.path.join("shipped.txt"), "yes\n").unwrap();
        commit_all(&worktree.path, "Ship it").await.unwrap();
        assert!(!is_merged(&repo, &worktree.branch, "main").await.unwrap());

        merge_branch(&repo, &worktree.branch, "main").await.unwrap();
        assert!(is_merged(&repo, &worktree.branch, "main").await.unwrap());

        remove_worktree(&repo, &worktree.path).await.unwrap();
        let _ = std::fs::remove_dir_all(&repo);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn changed_files_report_status_and_counts() {
        let repo = fixture_repo("files").await;
        let root = repo.join("..").join("heretic-worktrees-files");
        let worktree = create_worktree(&repo, &root, "task10", "Touch things", None)
            .await
            .unwrap();

        std::fs::write(worktree.path.join("README.md"), "hello\nworld\n").unwrap();
        std::fs::write(worktree.path.join("fresh.txt"), "one\ntwo\nthree\n").unwrap();

        let scope = DiffScope::working_tree(&worktree.path, "main");
        let files = changed_files(&scope).await.unwrap();

        let readme = files.iter().find(|f| f.path == "README.md").unwrap();
        assert_eq!(readme.status, ChangeStatus::Modified);
        assert_eq!(readme.insertions, 1);

        // An untracked file is part of the work even though git has not seen it.
        let fresh = files.iter().find(|f| f.path == "fresh.txt").unwrap();
        assert_eq!(fresh.status, ChangeStatus::Untracked);
        assert_eq!(fresh.insertions, 3);
        assert!(!fresh.binary);

        // Both are readable as patches, tracked or not.
        assert!(file_diff(&scope, "README.md", 10_000)
            .await
            .unwrap()
            .contains("+world"));
        assert!(file_diff(&scope, "fresh.txt", 10_000)
            .await
            .unwrap()
            .contains("+three"));

        remove_worktree(&repo, &worktree.path).await.unwrap();
        let _ = std::fs::remove_dir_all(&repo);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn a_rename_keeps_the_path_it_came_from() {
        let repo = fixture_repo("rename").await;
        let root = repo.join("..").join("heretic-worktrees-rename");
        let worktree = create_worktree(&repo, &root, "task11", "Move a file", None)
            .await
            .unwrap();

        git(&worktree.path, &["mv", "README.md", "GUIDE.md"])
            .await
            .unwrap();

        let scope = DiffScope::working_tree(&worktree.path, "main");
        let files = changed_files(&scope).await.unwrap();

        let moved = files.iter().find(|f| f.path == "GUIDE.md").unwrap();
        assert_eq!(moved.status, ChangeStatus::Renamed);
        assert_eq!(moved.old_path.as_deref(), Some("README.md"));

        remove_worktree(&repo, &worktree.path).await.unwrap();
        let _ = std::fs::remove_dir_all(&repo);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn history_is_readable_before_and_after_a_merge() {
        let repo = fixture_repo("history").await;
        let root = repo.join("..").join("heretic-worktrees-history");
        let worktree = create_worktree(&repo, &root, "task12", "Two commits", None)
            .await
            .unwrap();

        std::fs::write(worktree.path.join("first.txt"), "one\n").unwrap();
        commit_all(&worktree.path, "Add the first file")
            .await
            .unwrap();
        std::fs::write(worktree.path.join("second.txt"), "two\n").unwrap();
        commit_all(&worktree.path, "Add the second file")
            .await
            .unwrap();

        let scope = DiffScope::working_tree(&worktree.path, "main");
        let log = commits(&scope, 50).await.unwrap();
        assert_eq!(log.len(), 2);
        // Newest first, the way the interface lists them.
        assert_eq!(log[0].subject, "Add the second file");
        assert_eq!(log[0].files_changed, 1);
        assert_eq!(log[0].insertions, 1);
        assert!(is_commit_id(&log[0].sha));

        let patch = commit_diff(&worktree.path, &log[0].sha, 10_000)
            .await
            .unwrap();
        assert!(patch.contains("second.txt"));

        // After merging, the worktree is gone but the branch is still readable
        // from the project's own checkout — the run's history must survive that.
        merge_branch(&repo, &worktree.branch, "main").await.unwrap();
        remove_worktree(&repo, &worktree.path).await.unwrap();

        let merged = DiffScope::branch(&repo, "main", &worktree.branch);
        assert_eq!(commits(&merged, 50).await.unwrap().len(), 2);
        assert_eq!(changed_files(&merged).await.unwrap().len(), 2);

        let _ = std::fs::remove_dir_all(&repo);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn only_hex_commit_ids_are_accepted() {
        assert!(is_commit_id("a1b2c3d"));
        assert!(!is_commit_id("--upload-pack=evil"));
        assert!(!is_commit_id("HEAD"));
        assert!(!is_commit_id("abc"));
    }

    #[tokio::test]
    async fn a_non_repository_is_rejected() {
        let dir = std::env::temp_dir().join(format!("heretic-not-a-repo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(!is_repository(&dir).await);
        assert!(matches!(
            current_branch(&dir).await,
            Err(GitError::NotARepository { .. })
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_diff_is_produced_for_the_reviewer() {
        let repo = fixture_repo("diff").await;
        let root = repo.join("..").join("heretic-worktrees-diff");
        let worktree = create_worktree(&repo, &root, "task6", "Change", None)
            .await
            .unwrap();

        std::fs::write(worktree.path.join("README.md"), "changed\n").unwrap();
        let diff = full_diff(&worktree.path, "main", 10_000).await.unwrap();
        assert!(diff.contains("README.md"));
        assert!(diff.contains("changed"));

        remove_worktree(&repo, &worktree.path).await.unwrap();
        let _ = std::fs::remove_dir_all(&repo);
        let _ = std::fs::remove_dir_all(&root);
    }
}
