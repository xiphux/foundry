use anyhow::Result;

use crate::git;

pub fn run(ctx: &mut super::WorkflowCtx, name: &str, stat: bool) -> Result<()> {
    let workspace = ctx.workspace(name)?;
    let source_path = ctx.source_path;
    let worktree_path = &workspace.worktree_path;
    let branch = &workspace.branch;

    let main_branch = git::detect_main_branch(source_path)?;

    // Gather data. One porcelain call answers both "is anything uncommitted?"
    // and "what changed?" — asking separately ran the identical command twice.
    let commit_log = git::log_commits(source_path, &main_branch, branch).unwrap_or_default();
    let commit_count = commit_log.lines().filter(|l| !l.is_empty()).count();
    let porcelain = git::status_porcelain(worktree_path).unwrap_or_default();
    let uncommitted_status = !porcelain.is_empty();

    // Header
    if commit_count == 0 && !uncommitted_status {
        println!("No changes in workspace '{name}'.");
        return Ok(());
    }

    let mut header_parts = Vec::new();
    if commit_count > 0 {
        let s = if commit_count == 1 { "" } else { "s" };
        header_parts.push(format!("{commit_count} commit{s} ahead of {main_branch}"));
    }
    if uncommitted_status {
        header_parts.push("uncommitted changes".into());
    }
    println!("{name} ({}):\n", header_parts.join(" + "));

    // Commits section
    if commit_count > 0 {
        println!("Commits:");
        for line in commit_log.lines() {
            if !line.is_empty() {
                println!("  {line}");
            }
        }
        println!();
    }

    // Uncommitted section
    if uncommitted_status {
        println!("Uncommitted:");
        for line in porcelain.lines() {
            if !line.is_empty() {
                println!("  {line}");
            }
        }
        println!();
    }

    // Diff output, streamed rather than buffered — a large branch diff would
    // otherwise be held in memory several times over before anything printed.
    if commit_count > 0 {
        git::stream_diff_committed(source_path, &main_branch, branch, stat)?;
    }
    if uncommitted_status {
        if commit_count > 0 {
            println!();
        }
        git::stream_diff_uncommitted(worktree_path, stat)?;
    }

    Ok(())
}
