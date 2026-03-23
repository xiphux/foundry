use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use crate::agent_hooks;
use crate::config::{self, MergeStrategy, ResolvedConfig, TemplateVars};
use crate::git;
use crate::history;
use crate::state::WorkspaceState;
use crate::terminal;

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

    if git::has_uncommitted_changes(source_path)? {
        anyhow::bail!(
            "main repo at '{}' has uncommitted changes. Commit or stash them before finishing.",
            source_path.display()
        );
    }

    let template_vars = TemplateVars {
        source: source_path.to_string_lossy().into(),
        worktree: worktree_path.to_string_lossy().into(),
        branch: branch.clone(),
        name: name.into(),
        project: project_name.into(),
    };

    for script in &config.teardown_scripts {
        let resolved_command = config::resolve_template(&script.command, &template_vars)?;
        let working_dir = if let Some(ref wd) = script.working_dir {
            config::resolve_template(wd, &template_vars)?
        } else {
            worktree_path.to_string_lossy().into()
        };

        if verbose {
            eprintln!("Running teardown script: {}...", script.name);
        }

        let status = Command::new("sh")
            .arg("-c")
            .arg(&resolved_command)
            .current_dir(&working_dir)
            .status()
            .with_context(|| format!("failed to run teardown script '{}'", script.name))?;

        if !status.success() {
            eprintln!(
                "Warning: teardown script '{}' failed (exit code {}), continuing...",
                script.name,
                status.code().unwrap_or(-1)
            );
        }
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

    if verbose {
        eprintln!("Removing worktree...");
    }
    git::remove_worktree(source_path, &worktree_path, false)?;

    // Archive if the branch had commits, otherwise just delete it
    if has_commits {
        if verbose {
            eprintln!("Archiving branch '{branch}'...");
        }
        git::archive_branch(source_path, &branch, &config.archive_prefix)?;
        eprintln!("Finished workspace '{name}'. Branch '{branch}' archived.");
    } else {
        if verbose {
            eprintln!("Deleting branch '{branch}' (no commits)...");
        }
        git::delete_branch(source_path, &branch)?;
        eprintln!("Finished workspace '{name}'. Branch '{branch}' deleted (no commits).");
    }

    let strategy_str = match config.merge_strategy {
        MergeStrategy::FfOnly => "ff-only",
        MergeStrategy::Merge => "merge",
    };
    let _ = history::record(&history::HistoryEvent::finished(
        project_name,
        name,
        &branch,
        commit_count,
        strategy_str,
    ));

    state.remove(project_name, name);
    state.save_to(state_path)?;
    agent_hooks::remove_status(project_name, name);

    // Close the terminal tab LAST — if we're running from inside the worktree's
    // tab, this will kill our own process. All cleanup must be done before this.
    if !tab_id.is_empty() {
        if verbose {
            eprintln!("Closing terminal tab...");
        }
        if let Ok(backend) = terminal::detect_terminal() {
            let _ = backend.close_tab(&tab_id);
        }
    }

    Ok(())
}
