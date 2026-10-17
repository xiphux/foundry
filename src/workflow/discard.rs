use anyhow::Result;

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
    // A branch deleted by hand has nothing left to lose and nothing to count.
    let commit_count = if git::branch_exists(source_path, &ws.branch)? {
        let main_branch = git::detect_main_branch(source_path)?;
        let base = git::main_start_point(source_path, &main_branch, &ctx.config.fetch_remote)?;
        git::commit_count(source_path, &ws.branch, &base)?
    } else {
        0
    };
    if commit_count > 0 && !force {
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
        && !crate::prompt::confirm("Worktree has uncommitted changes. Discard anyway?")?
    {
        return Err(crate::prompt::Aborted.into());
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
        super::BranchCleanup::Archive {
            commits: commit_count,
        },
        // Discard exists to throw work away, so a dirty worktree is expected
        // here — unlike finish, which refuses to run on one.
        true,
        |archived_as| {
            history::HistoryEvent::discarded(&project, name, &branch, commit_count, archived_as)
        },
        // Announced from inside the cleanup rather than before it, so the
        // message is only printed once the branch has actually been dealt with.
        // `archived_as` is the outcome itself, so this reports what happened
        // instead of re-deriving it from the count.
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
