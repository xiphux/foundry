use anyhow::Result;
use std::path::Path;

use crate::config::ResolvedConfig;
use crate::git;
use crate::state::WorkspaceState;

pub fn run(
    name: &str,
    project_name: &str,
    source_path: &Path,
    config: &ResolvedConfig,
    state: &WorkspaceState,
    stat: bool,
) -> Result<()> {
    let worktree_path = config.worktree_dir.join(project_name).join(name);
    if !worktree_path.exists() {
        anyhow::bail!(
            "worktree '{name}' does not exist at {}",
            worktree_path.display()
        );
    }

    // Look up branch name from state
    let workspace = state
        .find_by_worktree_path(&worktree_path.to_string_lossy())
        .ok_or_else(|| anyhow::anyhow!("workspace '{name}' not found in state"))?;
    let branch = &workspace.branch;

    let main_branch = git::detect_main_branch(source_path)?;

    // Gather data
    let commit_log = git::log_commits(source_path, &main_branch, branch).unwrap_or_default();
    let commit_count = commit_log.lines().filter(|l| !l.is_empty()).count();
    let uncommitted_status = git::has_uncommitted_changes(&worktree_path).unwrap_or(false);

    // Header
    if commit_count == 0 && !uncommitted_status {
        println!("No changes in workspace '{name}'.");
        return Ok(());
    }

    let mut header_parts = Vec::new();
    if commit_count > 0 {
        let s = if commit_count == 1 { "" } else { "s" };
        header_parts.push(format!("{commit_count} commit{s} ahead of {main_branch}"));
    }
    if uncommitted_status {
        header_parts.push("uncommitted changes".into());
    }
    println!("{name} ({}):\n", header_parts.join(" + "));

    // Commits section
    if commit_count > 0 {
        println!("Commits:");
        for line in commit_log.lines() {
            if !line.is_empty() {
                println!("  {line}");
            }
        }
        println!();
    }

    // Uncommitted section
    if uncommitted_status {
        let porcelain = git::status_porcelain(&worktree_path).unwrap_or_default();
        println!("Uncommitted:");
        for line in porcelain.lines() {
            if !line.is_empty() {
                println!("  {line}");
            }
        }
        println!();
    }

    // Diff output
    let committed_diff =
        git::diff_committed(source_path, &main_branch, branch, stat).unwrap_or_default();
    let uncommitted_diff = git::diff_uncommitted(&worktree_path, stat).unwrap_or_default();

    if !committed_diff.is_empty() || !uncommitted_diff.is_empty() {
        if !committed_diff.is_empty() {
            println!("{committed_diff}");
        }
        if !uncommitted_diff.is_empty() {
            if !committed_diff.is_empty() {
                println!();
            }
            println!("{uncommitted_diff}");
        }
    }

    Ok(())
}
