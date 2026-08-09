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
    let ws = ctx.workspace(name)?;
    let source_path = ctx.source_path;

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

    if git::has_uncommitted_changes(&ws.worktree_path)? && !skip_confirm && !force {
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
        eprintln!(
            "Discarded workspace '{name}'. Branch '{}' archived.",
            ws.branch
        );
    } else {
        eprintln!(
            "Discarded workspace '{name}'. Branch '{}' deleted (no commits).",
            ws.branch
        );
    }

    let project = ctx.project.to_string();
    let branch = ws.branch.clone();
    super::cleanup_workspace(
        ctx,
        &ws,
        super::BranchCleanup::Archive,
        // Discard exists to throw work away, so a dirty worktree is expected
        // here — unlike finish, which refuses to run on one.
        true,
        |archived_as| {
            history::HistoryEvent::discarded(&project, name, &branch, commit_count, archived_as)
        },
    )
}
