use anyhow::{Context, Result};
use std::path::Path;

use crate::git;
use crate::history;

/// Derive a short worktree name from an archived branch name.
/// e.g., "archive/xiphux/my-feature-20260321" → "my-feature"
/// Strips the archive prefix, optional branch prefix, and date suffix.
fn derive_worktree_name(branch: &str, archive_prefix: &str, branch_prefix: Option<&str>) -> String {
    // Strip archive prefix (e.g., "archive/")
    let without_archive = branch
        .strip_prefix(&format!("{archive_prefix}/"))
        .unwrap_or(branch);

    // Strip branch prefix if configured (e.g., "xiphux/")
    let without_branch_prefix = match branch_prefix {
        Some(bp) if !bp.is_empty() => without_archive
            .strip_prefix(&format!("{bp}/"))
            .unwrap_or(without_archive),
        _ => without_archive,
    };

    // Strip date suffix (e.g., "-20260321" or "-20260321-143052")
    // Look for a trailing -YYYYMMDD or -YYYYMMDD-HHMMSS pattern
    let name = without_branch_prefix;
    if let Some(pos) = name.rfind('-') {
        let suffix = &name[pos + 1..];
        // Check if it looks like a date (8 digits) or datetime (8-6 digits)
        if suffix.len() == 8 && suffix.chars().all(|c| c.is_ascii_digit()) {
            return name[..pos].to_string();
        }
        if suffix.len() == 6 && suffix.chars().all(|c| c.is_ascii_digit()) {
            // This is the HHMMSS part, strip it and check for YYYYMMDD before it
            let before_time = &name[..pos];
            if let Some(pos2) = before_time.rfind('-') {
                let date_part = &before_time[pos2 + 1..];
                if date_part.len() == 8 && date_part.chars().all(|c| c.is_ascii_digit()) {
                    return before_time[..pos2].to_string();
                }
            }
        }
    }

    name.to_string()
}

/// List archived branches for a project.
pub fn list_archived(source_path: &Path, archive_prefix: &str) -> Result<()> {
    let branches = git::list_branches_with_prefix(source_path, &format!("{archive_prefix}/"))?;

    if branches.is_empty() {
        println!("No archived branches found.");
        return Ok(());
    }

    println!("Archived branches:");
    for branch in &branches {
        println!("  {branch}");
    }

    Ok(())
}

/// Restore a workspace from an archived branch.
pub fn run(ctx: &mut super::WorkflowCtx, branch: &str) -> Result<()> {
    let (project_name, source_path, config, verbose) =
        (ctx.project, ctx.source_path, ctx.config, ctx.verbose);
    // Verify the branch exists
    if !git::branch_exists(source_path, branch)? {
        // Try prepending the archive prefix if not already present
        let with_prefix = format!("{}/{branch}", config.archive_prefix);
        if git::branch_exists(source_path, &with_prefix)? {
            return run(ctx, &with_prefix);
        }
        anyhow::bail!(
            "branch '{branch}' not found. Run `foundry restore` with no arguments to see archived branches."
        );
    }

    // Derive a worktree name from the branch
    let name = derive_worktree_name(
        branch,
        &config.archive_prefix,
        config.branch_prefix.as_deref(),
    );
    super::validate_workspace_name(&name)
        .with_context(|| format!("branch '{branch}' does not map to a usable workspace name"))?;
    let worktree_path = config.worktree_dir.join(project_name).join(&name);

    // Check if worktree already exists
    if worktree_path.exists() {
        if verbose {
            eprintln!(
                "Worktree already exists at {}, opening workspace...",
                worktree_path.display()
            );
        }
        super::open::open_workspace(ctx, &name, &worktree_path, &Default::default())?;
        return Ok(());
    }

    // The same checks and order as `start`, so a refusal leaves nothing behind.
    // Prior conversations are kept, unlike in `start`: this is the same work
    // coming back, and resuming its agent session is the point.
    super::start::check_launchable(config)?;
    let allocated_ports = super::start::allocate_workspace_ports(ctx)?;

    // Create worktree directory parent
    if let Some(parent) = worktree_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }

    // Create worktree from the archived branch
    if verbose {
        eprintln!(
            "Creating worktree at {} from branch '{branch}'...",
            worktree_path.display()
        );
    }
    git::create_worktree(source_path, &worktree_path, branch)
        .with_context(|| "failed to create worktree")?;

    // Record history event
    let _ = history::record(&history::HistoryEvent::restored(
        project_name,
        &name,
        branch,
        branch, // from_branch is the archived branch name
    ));

    super::start::record_workspace(ctx, &name, branch, &worktree_path, allocated_ports)?;
    eprintln!("Restored workspace '{name}' from branch '{branch}'.");
    super::start::set_up_and_open(ctx, &name, branch, &worktree_path, None, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_worktree_name_with_date() {
        assert_eq!(
            derive_worktree_name("archive/my-feature-20260321", "archive", None),
            "my-feature"
        );
    }

    #[test]
    fn test_derive_worktree_name_with_datetime() {
        assert_eq!(
            derive_worktree_name("archive/my-feature-20260321-143052", "archive", None),
            "my-feature"
        );
    }

    #[test]
    fn test_derive_worktree_name_with_branch_prefix() {
        assert_eq!(
            derive_worktree_name(
                "archive/xiphux/my-feature-20260321",
                "archive",
                Some("xiphux")
            ),
            "my-feature"
        );
    }

    #[test]
    fn test_derive_worktree_name_no_date() {
        assert_eq!(
            derive_worktree_name("archive/my-feature", "archive", None),
            "my-feature"
        );
    }

    #[test]
    fn test_derive_worktree_name_hyphenated_feature() {
        assert_eq!(
            derive_worktree_name("archive/fix-sentry-error-20260321", "archive", None),
            "fix-sentry-error"
        );
    }
}
