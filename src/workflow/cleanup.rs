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
/// **Important:** The terminal tab close is the last operation. If the caller is
/// running from inside the worktree's tab, closing the tab will kill the
/// process, so all state must be persisted first — and a caller cannot print
/// anything after this function returns. The one earlier close is in
/// `remove_worktree_resiliently`, which reaches it only when the caller is
/// *not* inside the worktree, and so cannot be killed by it.
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

    // Whether this process is running from inside the worktree it is about to
    // remove — `foundry finish` typed in the workspace's own tab. It decides
    // whether closing that tab is an option before the removal: closing the tab
    // we are running in kills this process part-way through the cleanup.
    let inside_worktree = std::env::current_dir()
        .map(|cwd| cwd.starts_with(worktree_path))
        .unwrap_or(false);

    // On Windows, directories can't be deleted while a process has them as cwd
    if cfg!(windows) && worktree_present && inside_worktree {
        anyhow::bail!(
            "on Windows, you cannot clean up a workspace from inside its worktree \
             (the directory is locked by your shell). Close this tab first, then retry."
        );
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
        if let Removal::Leftovers(git_err) = remove_worktree_resiliently(
            source_path,
            worktree_path,
            tab_id,
            inside_worktree,
            force_remove,
            verbose,
        )? {
            eprintln!(
                "Warning: could not delete the worktree directory '{}' ({git_err}).\n\
                 Something is still writing into it — a dev server left running in one \
                 of the panes is the usual cause.\n\
                 Git's registration for the worktree is already gone, so cleanup is \
                 continuing; stop those processes and `rm -rf` the directory at your \
                 convenience.",
                worktree_path.display()
            );
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

/// What became of the worktree directory.
enum Removal {
    /// Registration and directory are both gone.
    Done,
    /// Git's registration is gone, but files on disk could not be deleted.
    /// Carries git's own error for the report.
    Leftovers(String),
}

/// Remove the worktree, working around a directory that a running process keeps
/// repopulating.
///
/// The failure this exists for: a pane running a dev server recreates files
/// while `git worktree remove` is deleting the tree — Vite writes into
/// `node_modules/.vite`, and because those files are gitignored the clean check
/// never sees them — so the final `rmdir` fails with "Directory not empty".
///
/// Git deletes its administrative entry under `.git/worktrees/` anyway, so at
/// that point the worktree is *already gone* as far as git is concerned and a
/// second `git worktree remove` only reports that the path is not a working
/// tree. That is what used to strand the workspace: `finish` merged the branch,
/// then aborted on the removal, leaving the branch unarchived and the state
/// entry behind, recoverable only by deleting the directory by hand and running
/// `foundry discard`. So the leftover files are downgraded to a warning and the
/// rest of the cleanup runs.
///
/// A removal git *refused* — a dirty worktree, a locked one, a path it does not
/// own — is a different thing entirely: nothing has been deleted, and it stays
/// fatal. `git::worktree_registered` is what tells the two apart.
fn remove_worktree_resiliently(
    source_path: &std::path::Path,
    worktree_path: &std::path::Path,
    tab_id: &str,
    inside_worktree: bool,
    force: bool,
    verbose: bool,
) -> Result<Removal> {
    let Err(first_err) = git::remove_worktree(source_path, worktree_path, force) else {
        return Ok(Removal::Done);
    };

    // On a listing that cannot be read, assume the worktree is still registered:
    // that keeps the old, fatal behaviour rather than deleting a directory on a
    // guess.
    if git::worktree_registered(source_path, worktree_path).unwrap_or(true) {
        return Err(first_err);
    }

    // Whatever holds the directory open is most likely running in the
    // workspace's own panes, so close them — unless this process is one of
    // them, in which case the close would kill the cleanup it is trying to
    // finish.
    //
    // Best-effort, and on Ghostty often no effect at all: `confirm-close-surface`
    // is on by default, so a tab whose panes still have running processes — a
    // live dev server being exactly the case this path exists for — waits for
    // the user to confirm instead of closing. The sweep below and the
    // `Leftovers` warning are what actually carry that case.
    if !tab_id.is_empty() && !inside_worktree {
        if verbose {
            eprintln!("Worktree directory could not be deleted; closing the terminal tab...");
        }
        if let Ok(backend) = terminal::detect_terminal() {
            let _ = backend.close_tab(tab_id);
        }
        std::thread::sleep(std::time::Duration::from_millis(if cfg!(windows) {
            1500
        } else {
            500
        }));
    }

    // Sweep the leftovers. The first sweep often loses the same race git did —
    // the writer is still alive when it starts — so it is worth a few tries.
    for attempt in 0..3 {
        if !worktree_path.exists() {
            break;
        }
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(250 * attempt));
        }
        if std::fs::remove_dir_all(worktree_path).is_ok() {
            break;
        }
    }

    if worktree_path.exists() {
        return Ok(Removal::Leftovers(first_err.to_string()));
    }

    // The registration read above said there was nothing left to prune, but a
    // path that failed to match git's spelling of it would have read the same
    // way. Pruning costs one git call and closes that gap.
    let _ = git::prune_worktrees(source_path);
    Ok(Removal::Done)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use tempfile::TempDir;

    fn git(dir: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// A repo with one commit that ignores `node_modules/`, plus a worktree on
    /// its own branch. The ignore rule matters: it is what lets a dev server's
    /// cache sail past `git worktree remove`'s clean check.
    fn repo_with_worktree() -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("source");
        std::fs::create_dir(&source).unwrap();
        git(&source, &["init", "-q"]);
        git(&source, &["config", "user.email", "test@example.com"]);
        git(&source, &["config", "user.name", "test"]);
        std::fs::write(source.join(".gitignore"), "node_modules/\n").unwrap();
        git(&source, &["add", "-A"]);
        git(&source, &["commit", "-qm", "initial"]);

        let worktree = dir.path().join("wt");
        git(
            &source,
            &[
                "worktree",
                "add",
                "-q",
                worktree.to_str().unwrap(),
                "-b",
                "feat",
            ],
        );
        (dir, worktree)
    }

    #[test]
    fn removes_a_clean_worktree() {
        let (dir, worktree) = repo_with_worktree();
        let source = dir.path().join("source");

        let outcome = remove_worktree_resiliently(&source, &worktree, "", true, false, false);
        assert!(matches!(outcome, Ok(Removal::Done)));
        assert!(!worktree.exists());
        assert!(!git::worktree_registered(&source, &worktree).unwrap());
    }

    /// A removal git *refused* has deleted nothing, so it must stay fatal —
    /// the recovery path would otherwise delete a worktree full of work.
    #[test]
    fn a_refused_removal_stays_fatal() {
        let (dir, worktree) = repo_with_worktree();
        let source = dir.path().join("source");
        std::fs::write(worktree.join("wip.txt"), "uncommitted").unwrap();

        let outcome = remove_worktree_resiliently(&source, &worktree, "", true, false, false);
        assert!(outcome.is_err());
        assert!(worktree.join("wip.txt").exists());
        assert!(git::worktree_registered(&source, &worktree).unwrap());
    }

    /// Files git could not delete are a warning, not a failure: the
    /// registration is already gone, so the caller must be free to carry on and
    /// archive the branch and clear the state entry.
    ///
    /// An unwritable ignored directory stands in for the dev server that
    /// repopulates `node_modules/.vite` while git is deleting the tree — same
    /// end state, without racing a real process.
    #[cfg(unix)]
    #[test]
    fn leftover_files_are_reported_but_not_fatal() {
        use std::os::unix::fs::PermissionsExt as _;

        let (dir, worktree) = repo_with_worktree();
        let source = dir.path().join("source");
        let cache = worktree.join("node_modules/.vite");
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(cache.join("deps.json"), "{}").unwrap();
        std::fs::set_permissions(&cache, std::fs::Permissions::from_mode(0o555)).unwrap();

        let outcome = remove_worktree_resiliently(&source, &worktree, "", true, false, false);

        // Let TempDir clean up after itself before asserting.
        std::fs::set_permissions(&cache, std::fs::Permissions::from_mode(0o755)).unwrap();

        assert!(matches!(outcome, Ok(Removal::Leftovers(_))));
        assert!(cache.join("deps.json").exists());
        assert!(!git::worktree_registered(&source, &worktree).unwrap());
    }
}
