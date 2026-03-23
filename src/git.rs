use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

fn run_git(repo_path: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(args)
        .output()
        .context("failed to execute git")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git {} failed: {}", args.join(" "), stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn detect_main_branch(repo_path: &Path) -> Result<String> {
    if let Ok(output) = run_git(repo_path, &["symbolic-ref", "refs/remotes/origin/HEAD"]) {
        if let Some(branch) = output.strip_prefix("refs/remotes/origin/") {
            return Ok(branch.to_string());
        }
    }
    let branches = run_git(
        repo_path,
        &["branch", "--list", "--format=%(refname:short)"],
    )?;
    for candidate in ["main", "master"] {
        if branches.lines().any(|b| b == candidate) {
            return Ok(candidate.to_string());
        }
    }
    bail!("could not detect main branch: neither 'main' nor 'master' found")
}

pub fn create_branch(repo_path: &Path, name: &str) -> Result<()> {
    run_git(repo_path, &["branch", name])?;
    Ok(())
}

pub fn create_worktree(repo_path: &Path, worktree_path: &Path, branch: &str) -> Result<()> {
    let path_str = worktree_path.to_str().context("invalid worktree path")?;
    run_git(repo_path, &["worktree", "add", path_str, branch])?;
    Ok(())
}

pub fn remove_worktree(repo_path: &Path, worktree_path: &Path, force: bool) -> Result<()> {
    let path_str = worktree_path.to_str().context("invalid worktree path")?;
    let mut args = vec!["worktree", "remove", path_str];
    if force {
        args.push("--force");
    }
    run_git(repo_path, &args)?;
    Ok(())
}

/// Fetch from a remote.
pub fn fetch(repo_path: &Path, remote: &str) -> Result<()> {
    run_git(repo_path, &["fetch", remote])?;
    Ok(())
}

/// Fast-forward the current branch to match a remote tracking branch.
/// Fails if the merge is not a fast-forward (e.g., local and remote have diverged).
pub fn ff_to_remote(repo_path: &Path, remote: &str, branch: &str) -> Result<()> {
    let remote_ref = format!("{remote}/{branch}");
    run_git(repo_path, &["merge", "--ff-only", &remote_ref])?;
    Ok(())
}

pub fn merge_ff_only(repo_path: &Path, branch: &str) -> Result<()> {
    run_git(repo_path, &["merge", "--ff-only", branch])?;
    Ok(())
}

pub fn merge(repo_path: &Path, branch: &str) -> Result<()> {
    let result = run_git(repo_path, &["merge", branch]);
    if let Err(e) = result {
        let _ = run_git(repo_path, &["merge", "--abort"]);
        return Err(e);
    }
    Ok(())
}

/// Check if a branch has any commits that aren't on the base branch.
/// Returns true if the branch has unique commits worth preserving.
pub fn branch_has_commits(repo_path: &Path, branch: &str, base: &str) -> Result<bool> {
    let output = run_git(
        repo_path,
        &["rev-list", "--count", &format!("{base}..{branch}")],
    )?;
    let count: u64 = output.parse().unwrap_or(0);
    Ok(count > 0)
}

/// Delete a branch.
pub fn delete_branch(repo_path: &Path, branch: &str) -> Result<()> {
    run_git(repo_path, &["branch", "-D", branch])?;
    Ok(())
}

pub fn archive_branch(repo_path: &Path, branch: &str, prefix: &str) -> Result<()> {
    let date = chrono::Utc::now().format("%Y%m%d").to_string();
    let archived = format!("{prefix}/{branch}-{date}");

    let exists = run_git(repo_path, &["branch", "--list", &archived])
        .map(|out| !out.is_empty())
        .unwrap_or(false);

    let final_name = if exists {
        let datetime = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
        format!("{prefix}/{branch}-{datetime}")
    } else {
        archived
    };

    run_git(repo_path, &["branch", "-m", branch, &final_name])?;
    Ok(())
}

/// List branches matching a prefix (e.g., "archive/").
pub fn list_branches_with_prefix(repo_path: &Path, prefix: &str) -> Result<Vec<String>> {
    let pattern = format!("{prefix}*");
    let output = run_git(
        repo_path,
        &["branch", "--list", "--format=%(refname:short)", &pattern],
    )?;
    Ok(output
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect())
}

/// Check if a branch exists.
pub fn branch_exists(repo_path: &Path, name: &str) -> Result<bool> {
    let output = run_git(
        repo_path,
        &["branch", "--list", "--format=%(refname:short)", name],
    )?;
    Ok(!output.is_empty())
}

pub fn has_uncommitted_changes(repo_path: &Path) -> Result<bool> {
    let output = run_git(repo_path, &["status", "--porcelain"])?;
    Ok(!output.is_empty())
}

/// Get the porcelain status output listing changed files.
pub fn status_porcelain(repo_path: &Path) -> Result<String> {
    run_git(repo_path, &["status", "--porcelain"])
}

pub fn current_branch(repo_path: &Path) -> Result<String> {
    run_git(repo_path, &["rev-parse", "--abbrev-ref", "HEAD"])
}

/// Get the Unix timestamp of the most recent commit. Returns None if no commits.
pub fn last_commit_timestamp(repo_path: &Path) -> Result<Option<i64>> {
    let output = run_git(repo_path, &["log", "-1", "--format=%ct"]);
    match output {
        Ok(s) if !s.is_empty() => Ok(s.parse().ok()),
        _ => Ok(None),
    }
}

pub fn repo_root(path: &Path) -> Result<std::path::PathBuf> {
    let root = run_git(path, &["rev-parse", "--show-toplevel"])?;
    Ok(std::path::PathBuf::from(root))
}

/// Get the commit log between base and branch as one-line summaries.
pub fn log_commits(repo_path: &Path, base: &str, branch: &str) -> Result<String> {
    run_git(
        repo_path,
        &["log", "--oneline", &format!("{base}..{branch}")],
    )
}

/// Get the diff of committed changes between base and branch (three-dot merge-base diff).
/// If `stat` is true, returns `--stat` summary instead of full patch.
pub fn diff_committed(repo_path: &Path, base: &str, branch: &str, stat: bool) -> Result<String> {
    let range = format!("{base}...{branch}");
    if stat {
        run_git(repo_path, &["diff", "--stat", &range])
    } else {
        run_git(repo_path, &["diff", &range])
    }
}

/// Get the uncommitted changes (both staged and unstaged) in a worktree.
/// If `stat` is true, returns `--stat` summary instead of full patch.
pub fn diff_uncommitted(worktree_path: &Path, stat: bool) -> Result<String> {
    // Unstaged changes
    let unstaged = if stat {
        run_git(worktree_path, &["diff", "--stat"])?
    } else {
        run_git(worktree_path, &["diff"])?
    };

    // Staged changes
    let staged = if stat {
        run_git(worktree_path, &["diff", "--cached", "--stat"])?
    } else {
        run_git(worktree_path, &["diff", "--cached"])?
    };

    let mut parts = Vec::new();
    if !staged.is_empty() {
        parts.push(staged);
    }
    if !unstaged.is_empty() {
        parts.push(unstaged);
    }
    Ok(parts.join("\n"))
}
