//! Git worktree management.
//!
//! When several agents work one project at once they must not share a checkout —
//! two agents editing the same tree corrupt each other's work. Each task run gets
//! its own `git worktree` on its own branch, created off the project's base
//! branch and removed once the work has been integrated or discarded.
//!
//! Worktrees are created outside the user's repository (under Accelerate's data
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
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
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
/// which branches Accelerate created.
pub fn branch_name(task_id: &str, task_title: &str) -> String {
    let slug = slugify(task_title, 40);
    if slug.is_empty() {
        format!("accelerate/{task_id}")
    } else {
        format!("accelerate/{task_id}-{slug}")
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

/// Worktrees Accelerate created that git still knows about, used to clean up
/// after a crash.
pub async fn list_accelerate_worktrees(repo: &Path) -> Result<Vec<PathBuf>> {
    let output = git(repo, &["worktree", "list", "--porcelain"]).await?;
    let mut paths = Vec::new();
    let mut current: Option<PathBuf> = None;

    for line in output.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            current = Some(PathBuf::from(path.trim()));
        } else if let Some(branch) = line.strip_prefix("branch ") {
            if branch.contains("accelerate/") {
                if let Some(path) = current.take() {
                    paths.push(path);
                }
            }
        }
    }

    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a throwaway repository with one commit.
    async fn fixture_repo(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("accel-git-{name}-{}", std::process::id()));
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
            "accelerate/abc123-fix-the-login-bug"
        );
        // A title with nothing usable still produces a valid branch.
        assert_eq!(branch_name("abc123", "!!!"), "accelerate/abc123");
    }

    #[tokio::test]
    async fn a_worktree_is_created_on_its_own_branch() {
        let repo = fixture_repo("create").await;
        let root = repo.join("..").join("accel-worktrees-create");

        let worktree = create_worktree(&repo, &root, "task1", "Add a feature", None)
            .await
            .unwrap();

        assert!(worktree.path.exists());
        assert_eq!(worktree.branch, "accelerate/task1-add-a-feature");
        assert_eq!(worktree.base_branch, "main");
        assert_eq!(
            current_branch(&worktree.path).await.unwrap(),
            "accelerate/task1-add-a-feature"
        );

        remove_worktree(&repo, &worktree.path).await.unwrap();
        assert!(!worktree.path.exists());
        let _ = std::fs::remove_dir_all(&repo);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn changes_are_summarised_including_untracked_files() {
        let repo = fixture_repo("summary").await;
        let root = repo.join("..").join("accel-worktrees-summary");
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
        let root = repo.join("..").join("accel-worktrees-clean");
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
        let root = repo.join("..").join("accel-worktrees-merge");
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
        let root = repo.join("..").join("accel-worktrees-dirty");
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
        let root = repo.join("..").join("accel-worktrees-discard");
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
        let root = repo.join("..").join("accel-worktrees-unmerged");
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
        let root = repo.join("..").join("accel-worktrees-landed");
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
    async fn a_non_repository_is_rejected() {
        let dir = std::env::temp_dir().join(format!("accel-not-a-repo-{}", std::process::id()));
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
        let root = repo.join("..").join("accel-worktrees-diff");
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
