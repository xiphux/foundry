use anyhow::{Context, Result};
use std::path::Path;

use crate::agent_hooks;
use crate::config::{self, ResolvedConfig, TemplateVars};
use crate::git;
use crate::state::WorkspaceState;
use crate::terminal;

/// What to do with the local branch after cleanup.
pub enum BranchCleanup {
    /// Archive the branch if it has commits, otherwise delete it.
    Archive,
    /// Delete the local branch unconditionally (e.g., after PR merge
    /// where the remote branch was already deleted by the forge).
    Delete,
    /// Leave the branch as-is (no local branch cleanup needed).
    None,
}

/// Shared cleanup for finish and discard: teardown scripts, worktree removal,
/// branch cleanup, state persistence, and terminal tab close.
///
/// **Important:** The terminal tab close is always the last operation.
/// If the caller is running from inside the worktree's tab, closing the
/// tab will kill the process, so all state must be persisted first.
/// Print any user-visible success messages BEFORE calling this function.
#[allow(clippy::too_many_arguments)]
pub fn cleanup_workspace(
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
    branch_cleanup: BranchCleanup,
    history_event: &crate::history::HistoryEvent,
) -> Result<()> {
    let template_vars = TemplateVars {
        source: source_path.to_string_lossy().into(),
        worktree: worktree_path.to_string_lossy().into(),
        branch: branch.into(),
        name: name.into(),
        project: project_name.into(),
    };

    // Run teardown scripts
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

        let status = std::process::Command::new("sh")
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

    // On Windows, directories can't be deleted while a process has them as cwd
    if cfg!(windows) {
        let cwd = std::env::current_dir().unwrap_or_default();
        if cwd.starts_with(worktree_path) {
            anyhow::bail!(
                "on Windows, you cannot clean up a workspace from inside its worktree \
                 (the directory is locked by your shell). Close this tab first, then retry."
            );
        }
    }

    // Remove worktree
    if verbose {
        eprintln!("Removing worktree...");
    }
    if let Err(first_err) = git::remove_worktree(source_path, worktree_path, false) {
        if cfg!(windows) && !tab_id.is_empty() {
            if verbose {
                eprintln!("Worktree directory is locked, closing terminal panes and retrying...");
            }
            if let Ok(backend) = terminal::detect_terminal() {
                let _ = backend.close_tab(tab_id);
            }
            std::thread::sleep(std::time::Duration::from_millis(1500));

            if worktree_path.exists() {
                std::fs::remove_dir_all(worktree_path).with_context(|| {
                    format!(
                        "could not remove worktree directory after closing panes. \
                         Close any processes using '{}' and retry.",
                        worktree_path.display()
                    )
                })?;
            }
        } else {
            return Err(first_err);
        }
    }

    // Handle local branch cleanup
    match branch_cleanup {
        BranchCleanup::Archive => {
            let has_commits = git::branch_has_commits(
                source_path,
                branch,
                &git::detect_main_branch(source_path)?,
            )
            .unwrap_or(true);
            if has_commits {
                if verbose {
                    eprintln!("Archiving branch '{branch}'...");
                }
                git::archive_branch(source_path, branch, &config.archive_prefix)?;
            } else {
                if verbose {
                    eprintln!("Deleting branch '{branch}' (no commits)...");
                }
                git::delete_branch(source_path, branch)?;
            }
        }
        BranchCleanup::Delete => {
            if verbose {
                eprintln!("Deleting local branch '{branch}'...");
            }
            // Branch may already be gone (e.g., worktree removal cleaned it up).
            let _ = git::delete_branch(source_path, branch);
        }
        BranchCleanup::None => {}
    }

    // Record history
    let _ = crate::history::record(history_event);

    // Update state
    state.remove(project_name, name);
    state.save_to(state_path)?;
    agent_hooks::remove_status(project_name, name);

    // Close terminal tab LAST
    if !tab_id.is_empty() {
        if verbose {
            eprintln!("Closing terminal tab...");
        }
        if let Ok(backend) = terminal::detect_terminal() {
            let _ = backend.close_tab(tab_id);
        }
    }

    Ok(())
}
