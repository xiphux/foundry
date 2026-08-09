use anyhow::{Context, Result, bail};
use std::io::Write as _;
use std::path::Path;
use std::process::Command;

fn run_git(repo_path: &Path, args: &[&str]) -> Result<String> {
    run_git_inner(repo_path, args, false)
}

/// Run a read-only git command with `--no-optional-locks`.
///
/// `git status` normally refreshes the index and writes it back, which takes
/// `.git/index.lock`. Foundry polls status across every worktree (every 2s in
/// `status --watch`), and those worktrees have agents running git commands in
/// them — so the poll would contend for the lock and lose races against real
/// work. `--no-optional-locks` skips the writeback; the reported status is the
/// same.
fn run_git_readonly(repo_path: &Path, args: &[&str]) -> Result<String> {
    run_git_inner(repo_path, args, true)
}

fn run_git_inner(repo_path: &Path, args: &[&str], no_optional_locks: bool) -> Result<String> {
    let mut cmd = Command::new("git");
    if no_optional_locks {
        cmd.arg("--no-optional-locks");
    }
    let output = cmd
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
    if let Ok(output) = run_git(repo_path, &["symbolic-ref", "refs/remotes/origin/HEAD"])
        && let Some(branch) = output.strip_prefix("refs/remotes/origin/")
    {
        return Ok(branch.to_string());
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

/// Count the commits a branch has beyond base.
pub fn commit_count(repo_path: &Path, branch: &str, base: &str) -> Result<u64> {
    let output = run_git(
        repo_path,
        &["rev-list", "--count", &format!("{base}..{branch}")],
    )?;
    Ok(output.parse().unwrap_or(0))
}

/// Check if a branch has any commits that aren't on the base branch.
/// Returns true if the branch has unique commits worth preserving.
pub fn branch_has_commits(repo_path: &Path, branch: &str, base: &str) -> Result<bool> {
    Ok(commit_count(repo_path, branch, base)? > 0)
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

/// Check whether `relative` is tracked by git in this repo.
///
/// Used to tell a user's own local file apart from one the repository ships.
/// Errors (not a repo, git missing) report "not tracked", which is the safe
/// answer for callers that only relax behaviour for untracked files.
pub fn is_tracked(repo_path: &Path, relative: &str) -> bool {
    run_git(repo_path, &["ls-files", "--error-unmatch", "--", relative]).is_ok()
}

pub fn has_uncommitted_changes(repo_path: &Path) -> Result<bool> {
    let output = run_git_readonly(repo_path, &["status", "--porcelain"])?;
    Ok(!output.is_empty())
}

/// Check if a repo has uncommitted changes to tracked files only.
/// Unlike `has_uncommitted_changes`, this ignores untracked files —
/// useful for checking if a merge target is clean enough to proceed.
pub fn has_modified_tracked_files(repo_path: &Path) -> Result<bool> {
    let output = run_git_readonly(repo_path, &["status", "--porcelain", "-uno"])?;
    Ok(!output.is_empty())
}

/// Get the porcelain status output listing changed files.
pub fn status_porcelain(repo_path: &Path) -> Result<String> {
    run_git_readonly(repo_path, &["status", "--porcelain"])
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

/// Stream a git command's stdout straight through to ours.
///
/// Capturing output would materialize the whole patch in memory — once in the
/// child's pipe buffer, again as a String, and again for the lossy UTF-8
/// conversion — before a single byte reached the terminal. Diffs are unbounded
/// in size, so they are streamed instead.
fn stream_git(repo_path: &Path, args: &[&str]) -> Result<()> {
    // The child writes to fd 1 directly while our own stdout is buffered, so
    // anything already queued must be flushed or it lands after the diff.
    std::io::stdout().flush().ok();

    let status = Command::new("git")
        // Because the child inherits our stdout, git would see a tty and start
        // its pager, blocking until the user quits it — and `foundry diff` runs
        // up to three git commands, so that meant up to three pagers. Capturing
        // output used to suppress paging implicitly by handing git a pipe.
        .arg("--no-pager")
        .arg("-C")
        .arg(repo_path)
        .args(args)
        .status()
        .context("failed to execute git")?;

    if !status.success() {
        // A reader closing the pipe — quitting a pager, `foundry diff | head` —
        // kills git with SIGPIPE. That is the consumer saying "enough", not a
        // failure, so report it as a normal end of output rather than an error
        // on top of a perfectly good partial diff.
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt as _;
            const SIGPIPE: i32 = 13;
            if status.signal() == Some(SIGPIPE) {
                return Ok(());
            }
        }
        bail!("git {} failed", args.join(" "));
    }
    Ok(())
}

/// Stream the diff of committed changes between base and branch to stdout.
pub fn stream_diff_committed(repo_path: &Path, base: &str, branch: &str, stat: bool) -> Result<()> {
    let range = format!("{base}...{branch}");
    if stat {
        stream_git(repo_path, &["diff", "--stat", &range])
    } else {
        stream_git(repo_path, &["diff", &range])
    }
}

/// Stream the uncommitted changes (staged, then unstaged) in a worktree to stdout.
///
/// Both halves run unconditionally. Deciding up front which half has content
/// would mean re-deriving it from `git status --porcelain` text, and getting
/// that wrong silently drops the patch — so git is simply asked for both and
/// prints whichever is non-empty. The cost is one extra no-op subprocess.
pub fn stream_diff_uncommitted(worktree_path: &Path, stat: bool) -> Result<()> {
    if stat {
        stream_git(worktree_path, &["diff", "--cached", "--stat"])?;
        stream_git(worktree_path, &["diff", "--stat"])
    } else {
        stream_git(worktree_path, &["diff", "--cached"])?;
        stream_git(worktree_path, &["diff"])
    }
}

/// List all configured remote names.
pub fn list_remotes(repo_path: &Path) -> Result<Vec<String>> {
    let output = run_git(repo_path, &["remote"])?;
    Ok(output
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect())
}

/// Get the URL of a remote.
pub fn remote_url(repo_path: &Path, remote: &str) -> Result<String> {
    run_git(repo_path, &["remote", "get-url", remote])
}

/// Push a branch to a remote. Uses --set-upstream on first push.
pub fn push_branch(repo_path: &Path, remote: &str, branch: &str) -> Result<()> {
    run_git(repo_path, &["push", "-u", remote, branch])?;
    Ok(())
}
