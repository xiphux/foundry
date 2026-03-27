use anyhow::{Context, Result};
use std::path::Path;

use crate::config::{MergeStrategy, ResolvedConfig};
use crate::git;
use crate::history;
use crate::state::WorkspaceState;

pub fn run(
    name: &str,
    project_name: &str,
    source_path: &Path,
    config: &ResolvedConfig,
    state: &mut WorkspaceState,
    state_path: &Path,
    verbose: bool,
) -> Result<()> {
    let worktree_path = config.worktree_dir.join(project_name).join(name);

    if !worktree_path.exists() {
        anyhow::bail!(
            "worktree '{name}' does not exist at {}",
            worktree_path.display()
        );
    }

    let workspace = state
        .find_by_worktree_path(&worktree_path.to_string_lossy())
        .ok_or_else(|| anyhow::anyhow!("workspace '{name}' not found in state"))?;
    let branch = workspace.branch.clone();
    let tab_id = workspace.terminal_tab_id.clone();

    if git::has_uncommitted_changes(&worktree_path)? {
        anyhow::bail!(
            "worktree '{}' has uncommitted changes. Commit or stash them before finishing.",
            worktree_path.display()
        );
    }

    if git::has_modified_tracked_files(source_path)? {
        anyhow::bail!(
            "main repo at '{}' has uncommitted changes to tracked files. \
             Commit or stash them before finishing.",
            source_path.display()
        );
    }

    let main_branch = git::detect_main_branch(source_path)?;

    let current = git::current_branch(source_path)?;
    if current != main_branch {
        anyhow::bail!(
            "main repo is on branch '{current}', expected '{main_branch}'. \
             Checkout '{main_branch}' before finishing."
        );
    }

    // Check for commits BEFORE merging (after merge, branch matches main)
    let has_commits = git::branch_has_commits(source_path, &branch, &main_branch).unwrap_or(true);
    let commit_count = if has_commits {
        git::log_commits(source_path, &main_branch, &branch)
            .map(|log| log.lines().filter(|l| !l.is_empty()).count() as u64)
            .unwrap_or(0)
    } else {
        0
    };

    if verbose {
        eprintln!("Merging '{branch}' into '{main_branch}'...");
    }
    match config.merge_strategy {
        MergeStrategy::FfOnly => {
            git::merge_ff_only(source_path, &branch).with_context(|| {
                format!(
                    "fast-forward merge failed. Rebase '{branch}' onto '{main_branch}' first, \
                     then re-run `foundry finish {name}`."
                )
            })?;
        }
        MergeStrategy::Merge => {
            git::merge(source_path, &branch).with_context(|| {
                format!(
                    "merge failed due to conflicts. Resolve conflicts manually, \
                     then re-run `foundry finish {name}`."
                )
            })?;
        }
    }

    let strategy_str = match config.merge_strategy {
        MergeStrategy::FfOnly => "ff-only",
        MergeStrategy::Merge => "merge",
    };

    let history_event =
        history::HistoryEvent::finished(project_name, name, &branch, commit_count, strategy_str);

    // Print success message BEFORE cleanup — cleanup closes the terminal tab
    // as its last step, which kills the process if running from inside the tab.
    if has_commits {
        eprintln!("Finished workspace '{name}'. Branch '{branch}' archived.");
    } else {
        eprintln!("Finished workspace '{name}'. Branch '{branch}' deleted (no commits).");
    }

    super::cleanup_workspace(
        name,
        project_name,
        source_path,
        &worktree_path,
        &branch,
        &tab_id,
        config,
        state,
        state_path,
        verbose,
        super::BranchCleanup::Archive,
        &history_event,
    )?;

    Ok(())
}
