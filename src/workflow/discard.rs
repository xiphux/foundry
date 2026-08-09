use anyhow::Result;
use std::io::{self, Write};
use std::path::Path;

use crate::config::ResolvedConfig;
use crate::git;
use crate::history;
use crate::state::WorkspaceState;

#[allow(clippy::too_many_arguments)]
pub fn run(
    name: &str,
    project_name: &str,
    source_path: &Path,
    config: &ResolvedConfig,
    state: &mut WorkspaceState,
    state_path: &Path,
    verbose: bool,
    skip_confirm: bool,
    force: bool,
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

    // Check for unmerged commits — require --force to discard work
    let main_branch = git::detect_main_branch(source_path)?;
    let has_commits = git::branch_has_commits(source_path, &branch, &main_branch).unwrap_or(false);
    let commit_count = if has_commits {
        git::log_commits(source_path, &main_branch, &branch)
            .map(|log| log.lines().filter(|l| !l.is_empty()).count() as u64)
            .unwrap_or(0)
    } else {
        0
    };
    if has_commits && !force {
        let s = if commit_count == 1 { "" } else { "s" };
        anyhow::bail!(
            "branch '{branch}' has {commit_count} unmerged commit{s}. \
             Use `foundry discard {name} --force` to discard anyway, \
             or `foundry finish {name}` to merge first."
        );
    }

    if git::has_uncommitted_changes(&worktree_path)? && !skip_confirm && !force {
        print!("Worktree has uncommitted changes. Discard anyway? [y/N] ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            eprintln!("Aborted.");
            return Ok(());
        }
    }

    // Print the outcome BEFORE cleanup: the tab close at the end of
    // `cleanup_workspace` kills this process when run from inside the worktree.
    if has_commits {
        eprintln!("Discarded workspace '{name}'. Branch '{branch}' archived.");
    } else {
        eprintln!("Discarded workspace '{name}'. Branch '{branch}' deleted (no commits).");
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
        // Discard exists to throw work away, so a dirty worktree is expected
        // here — unlike finish, which refuses to run on one.
        true,
        |archived_as| {
            history::HistoryEvent::discarded(project_name, name, &branch, commit_count, archived_as)
        },
    )
}
