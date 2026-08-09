use anyhow::{Context, Result};
use chrono::Utc;
use std::path::Path;

use crate::config::TemplateVars;
use crate::git;
use crate::history;
use crate::state::Workspace;

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
    let (project_name, source_path, config, state_path, verbose) = (
        ctx.project,
        ctx.source_path,
        ctx.config,
        ctx.state_path,
        ctx.verbose,
    );
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
        return super::open::open_workspace(
            ctx,
            &name,
            &worktree_path,
            &std::collections::HashSet::new(),
            None, // no prompt for restore
            &std::collections::HashMap::new(),
            false,
        );
    }

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

    // Allocate ports for restored workspace
    let allocated_ports = if config.ports.is_empty() {
        std::collections::HashMap::new()
    } else {
        let reserved = ctx.state.all_allocated_ports();
        let ports = super::allocate_ports(&config.ports, &reserved, config.port_range_start);
        if verbose {
            let mut sorted: Vec<_> = ports.iter().collect();
            sorted.sort_by_key(|(_, v)| *v);
            for (name, port) in &sorted {
                eprintln!("Allocated port: {name}={port}");
            }
        }
        ports
    };

    // Record state before setup scripts
    ctx.state.add(Workspace {
        project: project_name.into(),
        name: name.clone(),
        branch: branch.into(),
        worktree_path: worktree_path.to_string_lossy().into(),
        source_path: source_path.to_string_lossy().into(),
        created_at: Utc::now(),
        terminal_tab_id: String::new(),
        allocated_ports,
        pr_number: None,
        pr_url: None,
    });
    ctx.state.save_to(state_path)?;

    // Run setup scripts
    let template_vars = TemplateVars {
        source: source_path.to_string_lossy().into(),
        worktree: worktree_path.to_string_lossy().into(),
        branch: branch.into(),
        name: name.clone(),
        project: project_name.into(),
    };

    // A deferred script is one the user asked to run *in a pane* — typically a
    // long-lived process like a dev server. Running it here would block
    // `restore` until the user killed it, so only the blocking phase runs.
    // (`start` chains the deferred ones into a pane; restore does not yet.)
    let (immediate, deferred): (Vec<_>, Vec<_>) =
        config.setup_scripts.iter().partition(|s| !s.deferred);
    if !deferred.is_empty() {
        eprintln!(
            "Note: {} deferred setup script(s) were not run — restore does not \
             start deferred scripts. Run them in the workspace if you need them.",
            deferred.len()
        );
    }

    // Same port environment `start` gives its setup scripts, read back from the
    // state entry written above.
    let script_env = ctx
        .state
        .find_by_worktree_path(&worktree_path.to_string_lossy())
        .map(|ws| ws.allocated_ports.clone())
        .unwrap_or_default();

    super::run_scripts(
        immediate.iter().copied(),
        super::ScriptKind::Setup,
        &template_vars,
        &script_env,
        verbose,
    )?;

    eprintln!("Restored workspace '{name}' from branch '{branch}'.");

    // Open workspace
    super::open::open_workspace(
        ctx,
        &name,
        &worktree_path,
        &std::collections::HashSet::new(),
        None, // no prompt for restore
        &std::collections::HashMap::new(),
        false,
    )
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
