use anyhow::{Context, Result};

use crate::forge;
use crate::git;
use crate::history;

pub fn run(
    ctx: &mut super::WorkflowCtx,
    name: &str,
    title: Option<&str>,
    body: Option<&str>,
) -> Result<()> {
    let workspace = ctx.workspace(name)?;
    let worktree_path = workspace.worktree_path;
    let branch = workspace.branch;
    let (source_path, config, verbose) = (ctx.source_path, ctx.config, ctx.verbose);

    // Check for uncommitted changes in the worktree
    if git::has_uncommitted_changes(&worktree_path)? {
        anyhow::bail!(
            "worktree '{}' has uncommitted changes. Commit or stash them before creating a PR.",
            worktree_path.display()
        );
    }

    // Detect forge from remote URL
    let (forge_impl, remote) = forge::detect_forge(source_path, config.pr_remote.as_deref())?;

    // Push the branch
    if verbose {
        eprintln!("Pushing '{branch}' to '{remote}'...");
    }
    git::push_branch(&worktree_path, &remote, &branch)
        .with_context(|| format!("failed to push branch '{branch}' to '{remote}'"))?;

    // Check if a PR already exists for this branch
    if let Some(existing) = forge_impl.pr_for_branch(source_path, &branch)? {
        // Link existing PR in state (may have been created manually on GitHub)
        ctx.state
            .set_pr_info(ctx.project, name, existing.number, &existing.url);
        ctx.state.save_to(ctx.state_path)?;
        eprintln!("Linked existing PR for branch '{branch}': {}", existing.url);
        return Ok(());
    }

    // Auto-generate title from branch name if not provided
    let pr_title = title.map(|s| s.to_string()).unwrap_or_else(|| {
        branch
            .rsplit('/')
            .next()
            .unwrap_or(&branch)
            .replace(['-', '_'], " ")
    });
    let pr_body = body.unwrap_or("");

    let main_branch = git::detect_main_branch(source_path)?;

    if verbose {
        eprintln!("Creating PR: {pr_title}...");
    }
    let pr_info = forge_impl.create_pr(source_path, &branch, &main_branch, &pr_title, pr_body)?;

    // Store PR info in workspace state
    ctx.state
        .set_pr_info(ctx.project, name, pr_info.number, &pr_info.url);
    ctx.state.save_to(ctx.state_path)?;

    let _ = history::record(&history::HistoryEvent::pr_created(
        ctx.project,
        name,
        &branch,
        pr_info.number,
        &pr_info.url,
    ));

    eprintln!("Created PR #{}: {}", pr_info.number, pr_info.url);

    Ok(())
}
