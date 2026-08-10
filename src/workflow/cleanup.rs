use anyhow::{Context, Result};

use crate::agent_hooks;
use crate::git;
use crate::terminal;

/// What to do with the local branch after cleanup.
pub enum BranchCleanup {
    /// Archive the branch if it has commits, otherwise delete it.
    ///
    /// `has_commits` carries a decision the caller already made. It exists for
    /// callers that change history before cleanup runs: `finish` merges the
    /// branch into main first, which makes `main..branch` empty, so recomputing
    /// here concluded "no commits" for a branch that had twenty — deleting it
    /// instead of archiving it, contradicting the archive rule in CLAUDE.md and
    /// leaving `foundry restore` with nothing to restore from.
    ///
    /// `None` recomputes, which is correct for `discard`: nothing has moved, so
    /// the count taken here is the true one.
    Archive { has_commits: Option<bool> },
    /// Delete the local branch unconditionally (e.g., after PR merge
    /// where the remote branch was already deleted by the forge).
    Delete,
    /// Leave the branch as-is (no local branch cleanup needed).
    None,
}

/// Shared cleanup for finish and discard: teardown scripts, worktree removal,
/// branch cleanup, state persistence, and terminal tab close.
///
/// `force_remove` is passed to `git worktree remove`. `finish` refuses to run
/// on a dirty worktree, so it never needs it; `discard` exists to throw work
/// away and always does.
///
/// `history_event` and `announce` are closures rather than values because both
/// may need to name the archived branch, which is only decided part-way through
/// this function — see `git::archive_branch`. Each receives `Some(name)` when
/// the branch was archived and `None` when it was deleted or left alone.
///
/// **Important:** The terminal tab close is always the last operation.
/// If the caller is running from inside the worktree's tab, closing the
/// tab will kill the process, so all state must be persisted first — and a
/// caller cannot print anything after this function returns.
///
/// `announce` is how a caller prints its success message despite that. It runs
/// after the branch operation, so the message is earned, and before the tab
/// close, so it still reaches the terminal. Callers used to print *before*
/// calling this function instead, which put the message ahead of the teardown
/// scripts, the worktree removal and the branch operation — so any failure in
/// between announced success and then reported an error, with nothing actually
/// cleaned up.
pub fn cleanup_workspace(
    ctx: &mut super::WorkflowCtx,
    ws: &super::ActiveWorkspace,
    branch_cleanup: BranchCleanup,
    force_remove: bool,
    history_event: impl FnOnce(Option<&str>) -> crate::history::HistoryEvent,
    announce: impl FnOnce(Option<&str>),
) -> Result<()> {
    let template_vars = ctx.template_vars(ws);
    let super::ActiveWorkspace {
        name,
        worktree_path,
        branch,
        terminal_tab_id: tab_id,
        ..
    } = ws;
    let (source_path, config, verbose) = (ctx.source_path, ctx.config, ctx.verbose);

    // The worktree directory can already be gone — deleted by hand, removed by
    // an external `git worktree remove`, or left behind by an earlier cleanup
    // that failed after removing it. Only `discard` can reach this function in
    // that state (`finish` resolves through the existence check), and it is
    // exactly the case discard needs to handle: git still has the worktree
    // registered, which keeps the branch checked out and the name unusable.
    let worktree_present = worktree_path.exists();

    // Run teardown scripts. They run *in* the worktree and act on it, so with
    // the directory gone there is nothing for them to tear down — and `sh -c`
    // with a missing `current_dir` fails to spawn at all, which would abort the
    // cleanup before it could clear anything.
    if worktree_present {
        super::run_scripts(
            &config.teardown_scripts,
            super::ScriptKind::Teardown,
            &template_vars,
            &std::collections::HashMap::new(),
            verbose,
        )?;
    } else if !config.teardown_scripts.is_empty() {
        eprintln!(
            "Warning: worktree directory {} is already gone; skipping {} teardown script(s).",
            worktree_path.display(),
            config.teardown_scripts.len()
        );
    }

    // On Windows, directories can't be deleted while a process has them as cwd
    if cfg!(windows) && worktree_present {
        let cwd = std::env::current_dir().unwrap_or_default();
        if cwd.starts_with(worktree_path) {
            anyhow::bail!(
                "on Windows, you cannot clean up a workspace from inside its worktree \
                 (the directory is locked by your shell). Close this tab first, then retry."
            );
        }
    }

    // Remove worktree
    if !worktree_present {
        // `git worktree remove` refuses a path that is not there, so the stale
        // registration has to be pruned instead. This must happen before branch
        // cleanup: while the registration exists git still counts the branch as
        // checked out and refuses to delete it.
        if verbose {
            eprintln!("Worktree directory already gone, pruning stale registration...");
        }
        git::prune_worktrees(source_path).with_context(|| {
            format!(
                "failed to prune the stale worktree registration for '{}'",
                worktree_path.display()
            )
        })?;
    } else {
        if verbose {
            eprintln!("Removing worktree...");
        }
        if let Err(first_err) = git::remove_worktree(source_path, worktree_path, force_remove) {
            if cfg!(windows) && !tab_id.is_empty() {
                if verbose {
                    eprintln!(
                        "Worktree directory is locked, closing terminal panes and retrying..."
                    );
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
                // The directory is gone but git may still hold the registration.
                let _ = git::prune_worktrees(source_path);
            } else {
                return Err(first_err);
            }
        }
    }

    // Handle local branch cleanup
    let archived_as = match branch_cleanup {
        BranchCleanup::Archive { has_commits } => {
            let has_commits = match has_commits {
                Some(decided) => decided,
                None => git::branch_has_commits(
                    source_path,
                    branch,
                    &git::detect_main_branch(source_path)?,
                )
                .unwrap_or(true),
            };
            if has_commits {
                if verbose {
                    eprintln!("Archiving branch '{branch}'...");
                }
                Some(git::archive_branch(
                    source_path,
                    branch,
                    &config.archive_prefix,
                )?)
            } else {
                if verbose {
                    eprintln!("Deleting branch '{branch}' (no commits)...");
                }
                git::delete_branch(source_path, branch)?;
                None
            }
        }
        BranchCleanup::Delete => {
            if verbose {
                eprintln!("Deleting local branch '{branch}'...");
            }
            // Branch may already be gone (e.g., worktree removal cleaned it up).
            let _ = git::delete_branch(source_path, branch);
            None
        }
        BranchCleanup::None => None,
    };

    // Record history
    let _ = crate::history::record(&history_event(archived_as.as_deref()));

    // Update state
    ctx.state.remove(ctx.project, name);
    ctx.state.save_to(ctx.state_path)?;

    // Every fallible step is done, so the caller's success message is now true;
    // the tab close, which can kill this process, is still ahead. Announcing
    // before the state write would have re-opened the same gap on a narrower
    // window — a failed save prints success and leaves the entry behind.
    announce(archived_as.as_deref());

    agent_hooks::remove_status(ctx.project, name);
    agent_hooks::remove_context(ctx.project, name);

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
