use anyhow::{Context, Result};

use crate::config::MergeStrategy;
use crate::forge;
use crate::git;
use crate::history;

pub fn run(
    ctx: &mut super::WorkflowCtx,
    name: &str,
    force_local: bool,
    skip_confirm: bool,
) -> Result<()> {
    let ws = ctx.workspace(name)?;

    if git::has_uncommitted_changes(&ws.worktree_path)? {
        anyhow::bail!(
            "worktree '{}' has uncommitted changes. Commit or stash them before finishing.",
            ws.worktree_path.display()
        );
    }

    // Decide: merge PR on GitHub, or merge locally?
    match ws.pr_number {
        Some(pr_num) if !force_local => do_pr_merge(ctx, &ws, pr_num, skip_confirm),
        Some(pr_num) => {
            // The PR link stays recorded until the merge succeeds, when cleanup
            // drops the whole entry. Clearing it first meant a merge that then
            // failed — dirty main, a non-fast-forward — forgot the PR for good.
            if ctx.verbose {
                eprintln!("Ignoring PR #{pr_num}, merging locally...");
            }
            do_local_merge(ctx, &ws)
        }
        None => {
            if force_local && ctx.verbose {
                eprintln!("No PR associated, merging locally...");
            }
            do_local_merge(ctx, &ws)
        }
    }
}

/// Merge the PR on GitHub, then clean up the workspace.
fn do_pr_merge(
    ctx: &mut super::WorkflowCtx,
    ws: &super::ActiveWorkspace,
    pr_number: u64,
    skip_confirm: bool,
) -> Result<()> {
    let (name, branch) = (ws.name.as_str(), ws.branch.as_str());
    let (source_path, config, verbose) = (ctx.source_path, ctx.config, ctx.verbose);
    let pr_url = ws.pr_url.as_deref();

    // Detect forge
    let (forge_impl, remote) = forge::detect_forge(source_path, config.pr_remote.as_deref())?;

    // Whether the PR is still open, and its CI status. Independent network
    // round trips to the forge, so they overlap rather than queue.
    let (live_pr, checks) = std::thread::scope(|s| {
        let checks = (!skip_confirm).then(|| s.spawn(|| forge_impl.pr_checks(source_path, branch)));
        let live_pr = forge_impl.pr_for_branch(source_path, branch);
        let checks = checks.map(|h| h.join().expect("PR checks thread panicked"));
        (live_pr, checks)
    });

    if live_pr?.is_some() {
        confirm_checks(pr_number, checks, verbose)?;

        if verbose {
            eprintln!("Merging PR #{pr_number} for branch '{branch}'...");
        }
        forge_impl.merge_pr(source_path, branch)?;

        // `gh pr merge` succeeds when it only adds the PR to a required merge
        // queue. Deleting the remote branch then would close the PR, and
        // cleanup would remove the only local copy of the work.
        if forge_impl.pr_state(source_path, pr_number)? != forge::PrState::Merged {
            eprintln!(
                "PR #{pr_number} was accepted but has not merged yet (a merge queue, \
                 perhaps). Workspace '{name}' is left as it is; run \
                 `foundry finish {name}` again once it has merged."
            );
            return Ok(());
        }
    } else if forge_impl.pr_state(source_path, pr_number)? == forge::PrState::Merged {
        // Merged since the last `finish` — through the queue, say — so all
        // that is left is the cleanup.
        if verbose {
            eprintln!("PR #{pr_number} has already merged.");
        }
    } else {
        let url_hint = pr_url.map(|u| format!(" ({u})")).unwrap_or_default();
        anyhow::bail!(
            "PR #{pr_number}{url_hint} for branch '{branch}' is no longer open.\n\
             Reopen the PR on GitHub to merge via PR, or run:\n  \
             foundry finish {name} --local\n\
             to merge locally instead."
        );
    }

    let history_event = history::HistoryEvent::pr_merged(ctx.project, name, branch, pr_number);

    // Best effort: a repository that auto-deletes head branches has already
    // done this, and the merge is what matters either way.
    if let Err(e) = git::delete_remote_branch(source_path, &remote, branch)
        && verbose
    {
        eprintln!("Remote branch '{branch}' not deleted: {e:#}");
    }

    // Fetch to update local refs after the remote merge
    let main_branch = git::detect_main_branch(source_path)?;
    if verbose {
        eprintln!("Fetching from '{remote}' to sync local refs...");
    }
    let _ = git::fetch(source_path, &remote);
    let _ = git::ff_to_remote(source_path, &remote, &main_branch);

    // Earned before cleanup starts: the PR is already merged on the forge, and
    // this message claims nothing about the local worktree or branch.
    eprintln!("Merged PR #{pr_number}.");

    super::cleanup_workspace(
        ctx,
        ws,
        super::BranchCleanup::Delete,
        // finish refuses to run on a dirty worktree, so a plain remove is
        // always enough here.
        false,
        |_| history_event,
        // Nothing further to announce — the merge is the outcome, and it has
        // already been reported.
        |_| {},
    )?;

    Ok(())
}

/// Ask before merging a PR whose CI has not passed. `checks` is `None` when
/// the user asked not to be prompted.
fn confirm_checks(
    pr_number: u64,
    checks: Option<Result<forge::ChecksStatus>>,
    verbose: bool,
) -> Result<()> {
    match checks {
        Some(Ok(status)) if !status.checks.is_empty() && !status.all_passed() => {
            super::checks::print_checks(pr_number, &status);
            let question = if status.has_failures() {
                "PR has failing checks. Merge anyway?"
            } else {
                "PR has pending checks. Merge anyway?"
            };
            if !crate::prompt::confirm(question)? {
                return Err(crate::prompt::Aborted.into());
            }
        }
        Some(Ok(status)) if verbose && !status.checks.is_empty() => {
            eprintln!("All {} checks passed.", status.checks.len());
        }
        Some(Err(e)) if verbose => {
            eprintln!("Warning: could not check CI status: {e}");
        }
        _ => {}
    }
    Ok(())
}

/// Merge the branch locally into main, then clean up the workspace.
fn do_local_merge(ctx: &mut super::WorkflowCtx, ws: &super::ActiveWorkspace) -> Result<()> {
    let (name, branch) = (ws.name.as_str(), ws.branch.as_str());
    let (source_path, config, verbose) = (ctx.source_path, ctx.config, ctx.verbose);

    if git::has_modified_tracked_files(source_path)? {
        anyhow::bail!(
            "main repo at '{}' has uncommitted changes to tracked files. \
             Commit or stash them before finishing.",
            source_path.display()
        );
    }

    let main_branch = git::detect_main_branch(source_path)?;

    // The merge below lands on HEAD, so HEAD has to be main. Checked before the
    // fetch so that a refusal leaves the repo untouched.
    let current = git::current_branch(source_path)?;
    if current != main_branch {
        anyhow::bail!(
            "main repo is on branch '{current}', expected '{main_branch}'. \
             Checkout '{main_branch}' before finishing."
        );
    }

    // Fetch and fast-forward main before merging (same logic as start)
    if config.auto_fetch {
        super::sync_main_from_remote(source_path, &config.fetch_remote, &main_branch, verbose)?;
    }

    let commit_count = git::commit_count(source_path, branch, &main_branch)?;

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

    let history_event = history::HistoryEvent::finished(
        ctx.project,
        name,
        branch,
        commit_count,
        config.merge_strategy.as_str(),
    );

    super::cleanup_workspace(
        ctx,
        ws,
        // Hand over the count taken *before* the merge. Merging the branch into
        // main empties `main..branch`, so letting cleanup recompute would read
        // every merged branch as commitless and delete it instead of archiving
        // it — which is what has been happening.
        super::BranchCleanup::Archive {
            commits: commit_count,
        },
        false,
        |_| history_event,
        // The merge into main has already happened by the time cleanup starts,
        // but the branch disposition has not — so the whole message is
        // announced from inside, once both are true.
        |archived_as| match archived_as {
            Some(archived) => {
                eprintln!("Finished workspace '{name}'. Branch '{branch}' archived as {archived}.")
            }
            None => {
                eprintln!("Finished workspace '{name}'. Branch '{branch}' deleted (no commits).")
            }
        },
    )?;

    Ok(())
}
