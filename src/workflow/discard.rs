use anyhow::Result;
use std::io::{self, Write};

use crate::git;
use crate::history;

pub fn run(
    ctx: &mut super::WorkflowCtx,
    name: &str,
    skip_confirm: bool,
    force: bool,
) -> Result<()> {
    // Resolved without requiring the worktree directory: a workspace whose
    // directory has already gone still has a git worktree registration, a
    // branch and a state entry to clean up, and discard is the only command
    // that can clear them.
    let ws = ctx.recorded_workspace(name)?;
    let source_path = ctx.source_path;
    let worktree_present = ws.worktree_path.exists();

    // Check for unmerged commits — require --force to discard work
    let main_branch = git::detect_main_branch(source_path)?;
    let has_commits =
        git::branch_has_commits(source_path, &ws.branch, &main_branch).unwrap_or(false);
    let commit_count = if has_commits {
        git::log_commits(source_path, &main_branch, &ws.branch)
            .map(|log| log.lines().filter(|l| !l.is_empty()).count() as u64)
            .unwrap_or(0)
    } else {
        0
    };
    if has_commits && !force {
        let s = if commit_count == 1 { "" } else { "s" };
        anyhow::bail!(
            "branch '{}' has {commit_count} unmerged commit{s}. \
             Use `foundry discard {name} --force` to discard anyway, \
             or `foundry finish {name}` to merge first.",
            ws.branch
        );
    }

    // Only meaningful when the directory is still there; with it gone there is
    // nothing left to lose, and asking git about a missing worktree errors.
    if worktree_present
        && git::has_uncommitted_changes(&ws.worktree_path)?
        && !skip_confirm
        && !force
    {
        print!("Worktree has uncommitted changes. Discard anyway? [y/N] ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            eprintln!("Aborted.");
            return Ok(());
        }
    }

    if !worktree_present {
        eprintln!(
            "Worktree directory {} is already gone — clearing the leftover branch and state.",
            ws.worktree_path.display()
        );
    }

    let project = ctx.project.to_string();
    let branch = ws.branch.clone();
    super::cleanup_workspace(
        ctx,
        &ws,
        // Recomputed inside cleanup: discard changes no history beforehand, so
        // the count taken there is the true one.
        super::BranchCleanup::Archive { has_commits: None },
        // Discard exists to throw work away, so a dirty worktree is expected
        // here — unlike finish, which refuses to run on one.
        true,
        |archived_as| {
            history::HistoryEvent::discarded(&project, name, &branch, commit_count, archived_as)
        },
        // Announced from inside the cleanup rather than before it, so the
        // message is only printed once the branch has actually been dealt with.
        // `archived_as` is the outcome itself, so this reports what happened
        // instead of re-deriving it from `has_commits` — which is computed with
        // the opposite error fallback and can disagree.
        |archived_as| match archived_as {
            Some(archived) => {
                eprintln!("Discarded workspace '{name}'. Branch '{branch}' archived as {archived}.")
            }
            None => {
                eprintln!("Discarded workspace '{name}'. Branch '{branch}' deleted (no commits).")
            }
        },
    )
}
