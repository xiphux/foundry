use anyhow::{Context, Result};
use std::path::Path;

use crate::config::ResolvedConfig;
use crate::forge;
use crate::git;
use crate::history;
use crate::state::WorkspaceState;

#[allow(clippy::too_many_arguments)]
pub fn run(
    name: &str,
    project_name: &str,
    source_path: &Path,
    config: &ResolvedConfig,
    state: &mut WorkspaceState,
    state_path: &Path,
    verbose: bool,
    title: Option<&str>,
    body: Option<&str>,
) -> Result<()> {
    let worktree_path = config.worktree_dir.join(project_name).join(name);

    if !worktree_path.exists() {
        anyhow::bail!(
            "worktree '{name}' does not exist at {}",
            worktree_path.display()
        );
    }

    let branch = state
        .find_by_worktree_path(&worktree_path.to_string_lossy())
        .ok_or_else(|| anyhow::anyhow!("workspace '{name}' not found in state"))?
        .branch
        .clone();

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
        state.set_pr_info(project_name, name, existing.number, &existing.url);
        state.save_to(state_path)?;
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
    state.set_pr_info(project_name, name, pr_info.number, &pr_info.url);
    state.save_to(state_path)?;

    let _ = history::record(&history::HistoryEvent::pr_created(
        project_name,
        name,
        &branch,
        pr_info.number,
        &pr_info.url,
    ));

    eprintln!("Created PR #{}: {}", pr_info.number, pr_info.url);

    Ok(())
}
