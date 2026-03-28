use anyhow::{Context, Result};
use std::path::Path;

use std::io::{self, Write as _};

use crate::config::{MergeStrategy, ResolvedConfig};
use crate::forge;
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
    force_local: bool,
    skip_confirm: bool,
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
    let pr_number = workspace.pr_number;
    let pr_url = workspace.pr_url.clone();

    if git::has_uncommitted_changes(&worktree_path)? {
        anyhow::bail!(
            "worktree '{}' has uncommitted changes. Commit or stash them before finishing.",
            worktree_path.display()
        );
    }

    // Decide: merge PR on GitHub, or merge locally?
    if let Some(pr_num) = pr_number {
        if force_local {
            // User explicitly chose local merge, clear PR info
            state.clear_pr_info(project_name, name);
            state.save_to(state_path)?;
            if verbose {
                eprintln!("Ignoring PR #{pr_num}, merging locally...");
            }
            do_local_merge(
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
            )
        } else {
            do_pr_merge(
                name,
                project_name,
                source_path,
                &worktree_path,
                &branch,
                &tab_id,
                pr_num,
                pr_url.as_deref(),
                config,
                state,
                state_path,
                verbose,
                skip_confirm,
            )
        }
    } else {
        if force_local && verbose {
            eprintln!("No PR associated, merging locally...");
        }
        do_local_merge(
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
        )
    }
}

/// Merge the PR on GitHub, then clean up the workspace.
#[allow(clippy::too_many_arguments)]
fn do_pr_merge(
    name: &str,
    project_name: &str,
    source_path: &Path,
    worktree_path: &Path,
    branch: &str,
    tab_id: &str,
    pr_number: u64,
    pr_url: Option<&str>,
    config: &ResolvedConfig,
    state: &mut WorkspaceState,
    state_path: &Path,
    verbose: bool,
    skip_confirm: bool,
) -> Result<()> {
    // Detect forge
    let (forge_impl, remote) = forge::detect_forge(source_path, config.pr_remote.as_deref())?;

    // Verify the PR is still open
    let live_pr = forge_impl.pr_for_branch(source_path, branch)?;
    if live_pr.is_none() {
        let url_hint = pr_url.map(|u| format!(" ({u})")).unwrap_or_default();
        anyhow::bail!(
            "PR #{pr_number}{url_hint} for branch '{branch}' is no longer open.\n\
             Reopen the PR on GitHub to merge via PR, or run:\n  \
             foundry finish {name} --local\n\
             to merge locally instead."
        );
    }

    // Check CI status before merging
    if !skip_confirm {
        match forge_impl.pr_checks(source_path, branch) {
            Ok(status) if !status.checks.is_empty() && !status.all_passed() => {
                super::checks::print_checks(pr_number, &status);
                if status.has_failures() {
                    eprint!("PR has failing checks. Merge anyway? [y/N] ");
                } else {
                    eprint!("PR has pending checks. Merge anyway? [y/N] ");
                }
                io::stderr().flush()?;
                let mut input = String::new();
                io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    eprintln!("Aborted.");
                    return Ok(());
                }
            }
            Ok(status) if verbose && !status.checks.is_empty() => {
                eprintln!("All {} checks passed.", status.checks.len());
            }
            Err(e) if verbose => {
                eprintln!("Warning: could not check CI status: {e}");
            }
            _ => {}
        }
    }

    if verbose {
        eprintln!("Merging PR #{pr_number} for branch '{branch}'...");
    }

    forge_impl.merge_pr(source_path, branch)?;

    let history_event = history::HistoryEvent::pr_merged(project_name, name, branch, pr_number);

    // Fetch to update local refs after the remote merge
    let main_branch = git::detect_main_branch(source_path)?;
    if verbose {
        eprintln!("Fetching from '{remote}' to sync local refs...");
    }
    let _ = git::fetch(source_path, &remote);
    let _ = git::ff_to_remote(source_path, &remote, &main_branch);

    // Print success BEFORE cleanup
    eprintln!("Merged PR #{pr_number}.");

    super::cleanup_workspace(
        name,
        project_name,
        source_path,
        worktree_path,
        branch,
        tab_id,
        config,
        state,
        state_path,
        verbose,
        super::BranchCleanup::Delete,
        &history_event,
    )?;

    Ok(())
}

/// Merge the branch locally into main, then clean up the workspace.
#[allow(clippy::too_many_arguments)]
fn do_local_merge(
    name: &str,
    project_name: &str,
    source_path: &Path,
    worktree_path: &Path,
    branch: &str,
    tab_id: &str,
    config: &ResolvedConfig,
    state: &mut WorkspaceState,
    state_path: &Path,
    verbose: bool,
) -> Result<()> {
    if git::has_modified_tracked_files(source_path)? {
        anyhow::bail!(
            "main repo at '{}' has uncommitted changes to tracked files. \
             Commit or stash them before finishing.",
            source_path.display()
        );
    }

    let main_branch = git::detect_main_branch(source_path)?;

    // Fetch and fast-forward main before merging (same logic as start)
    if config.auto_fetch {
        let remote = &config.fetch_remote;
        if verbose {
            eprintln!("Fetching from {remote}...");
        }
        git::fetch(source_path, remote)
            .with_context(|| format!("failed to fetch from remote '{remote}'"))?;

        if verbose {
            eprintln!("Fast-forwarding {main_branch} to {remote}/{main_branch}...");
        }
        git::ff_to_remote(source_path, remote, &main_branch).with_context(|| {
            format!(
                "could not fast-forward '{main_branch}' to '{remote}/{main_branch}'. \
                 Your local {main_branch} may have diverged from the remote. \
                 Resolve manually with `git pull` in your source repo, then retry."
            )
        })?;
    }

    let current = git::current_branch(source_path)?;
    if current != main_branch {
        anyhow::bail!(
            "main repo is on branch '{current}', expected '{main_branch}'. \
             Checkout '{main_branch}' before finishing."
        );
    }

    let has_commits = git::branch_has_commits(source_path, branch, &main_branch).unwrap_or(true);
    let commit_count = if has_commits {
        git::log_commits(source_path, &main_branch, branch)
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
            git::merge_ff_only(source_path, branch).with_context(|| {
                format!(
                    "fast-forward merge failed. Rebase '{branch}' onto '{main_branch}' first, \
                     then re-run `foundry finish {name}`."
                )
            })?;
        }
        MergeStrategy::Merge => {
            git::merge(source_path, branch).with_context(|| {
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
        history::HistoryEvent::finished(project_name, name, branch, commit_count, strategy_str);

    // Print success BEFORE cleanup
    if has_commits {
        eprintln!("Finished workspace '{name}'. Branch '{branch}' archived.");
    } else {
        eprintln!("Finished workspace '{name}'. Branch '{branch}' deleted (no commits).");
    }

    super::cleanup_workspace(
        name,
        project_name,
        source_path,
        worktree_path,
        branch,
        tab_id,
        config,
        state,
        state_path,
        verbose,
        super::BranchCleanup::Archive,
        &history_event,
    )?;

    Ok(())
}
