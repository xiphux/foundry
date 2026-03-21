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
    let branches = run_git(repo_path, &["branch", "--list", "--format=%(refname:short)"])?;
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

pub fn has_uncommitted_changes(repo_path: &Path) -> Result<bool> {
    let output = run_git(repo_path, &["status", "--porcelain"])?;
    Ok(!output.is_empty())
}

pub fn current_branch(repo_path: &Path) -> Result<String> {
    run_git(repo_path, &["rev-parse", "--abbrev-ref", "HEAD"])
}

pub fn repo_root(path: &Path) -> Result<std::path::PathBuf> {
    let root = run_git(path, &["rev-parse", "--show-toplevel"])?;
    Ok(std::path::PathBuf::from(root))
}
