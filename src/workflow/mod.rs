pub mod diff;
pub mod discard;
pub mod finish;
pub mod open;
pub mod restore;
pub mod start;
pub mod status;

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::agent_hooks;
use crate::config::{self, ResolvedConfig, TemplateVars};
use crate::git;
use crate::registry::Registry;
use crate::state::WorkspaceState;
use crate::terminal;

pub fn resolve_project(
    project_flag: Option<&str>,
    registry: &mut Registry,
    registry_path: &Path,
) -> Result<(String, PathBuf)> {
    if let Some(name) = project_flag {
        let path = registry.get(name).with_context(|| {
            format!("project '{name}' not found. Register it with `foundry projects add`.")
        })?;
        return Ok((name.to_string(), path));
    }

    let cwd = std::env::current_dir().context("failed to get current directory")?;
    let repo_root = git::repo_root(&cwd).context("not inside a git repository")?;

    if let Some(name) = registry.find_by_path(&repo_root) {
        return Ok((name, repo_root));
    }

    let name = repo_root
        .file_name()
        .context("repo root has no directory name")?
        .to_str()
        .context("directory name is not valid UTF-8")?
        .to_string();

    if registry.get(&name).is_some() {
        anyhow::bail!(
            "project name '{name}' is already registered to a different path. \
             Use `foundry projects add <custom-name> {}` to register with a different name.",
            repo_root.display()
        );
    }

    eprintln!(
        "Auto-registering project '{name}' at {}",
        repo_root.display()
    );
    registry.add(&name, repo_root.clone())?;
    registry.save_to(registry_path)?;

    Ok((name, repo_root))
}

pub fn compute_branch_name(name: &str, prefix: Option<&str>) -> String {
    match prefix {
        Some(p) if !p.is_empty() => format!("{p}/{name}"),
        _ => name.to_string(),
    }
}

/// Ending port for dynamic allocation range (exclusive).
const PORT_RANGE_END: u16 = 60000;

/// Allocate a contiguous block of ports for a new workspace.
/// Scans from `range_start` to find a contiguous block of `port_names.len()`
/// ports that don't overlap with any already-reserved ports.
pub fn allocate_ports(
    port_names: &[String],
    reserved: &[u16],
    range_start: u16,
) -> std::collections::HashMap<String, u16> {
    let count = port_names.len();
    if count == 0 {
        return std::collections::HashMap::new();
    }

    let mut sorted_reserved: Vec<u16> = reserved.to_vec();
    sorted_reserved.sort();

    // Find the first contiguous block of `count` ports in the range
    let mut start = range_start;
    'outer: while start + count as u16 <= PORT_RANGE_END {
        for offset in 0..count as u16 {
            let port = start + offset;
            if sorted_reserved.binary_search(&port).is_ok() {
                // This port is taken — skip past it
                start = port + 1;
                continue 'outer;
            }
        }
        // Found a contiguous block
        break;
    }

    port_names
        .iter()
        .enumerate()
        .map(|(i, name)| (name.clone(), start + i as u16))
        .collect()
}

pub fn foundry_paths() -> Result<(PathBuf, PathBuf)> {
    let foundry_dir = config::foundry_dir()?;
    Ok((
        foundry_dir.join("projects.toml"),
        foundry_dir.join("state.toml"),
    ))
}

/// What to do with the local branch after cleanup.
pub enum BranchCleanup {
    /// Archive the branch if it has commits, otherwise delete it.
    Archive,
    /// Delete the local branch unconditionally (e.g., after PR merge
    /// where the remote branch was already deleted by the forge).
    Delete,
    /// Leave the branch as-is (no local branch cleanup needed).
    None,
}

/// Shared cleanup for finish and merge: teardown scripts, worktree removal,
/// branch cleanup, state persistence, and terminal tab close.
///
/// **Important:** The terminal tab close is always the last operation.
/// If the caller is running from inside the worktree's tab, closing the
/// tab will kill the process, so all state must be persisted first.
/// Print any user-visible success messages BEFORE calling this function.
#[allow(clippy::too_many_arguments)]
pub fn cleanup_workspace(
    name: &str,
    project_name: &str,
    source_path: &Path,
    worktree_path: &Path,
    branch: &str,
    tab_id: &str,
    config: &ResolvedConfig,
    state: &mut WorkspaceState,
    state_path: &Path,
    verbose: bool,
    branch_cleanup: BranchCleanup,
    history_event: &crate::history::HistoryEvent,
) -> Result<()> {
    let template_vars = TemplateVars {
        source: source_path.to_string_lossy().into(),
        worktree: worktree_path.to_string_lossy().into(),
        branch: branch.into(),
        name: name.into(),
        project: project_name.into(),
    };

    // Run teardown scripts
    for script in &config.teardown_scripts {
        let resolved_command = config::resolve_template(&script.command, &template_vars)?;
        let working_dir = if let Some(ref wd) = script.working_dir {
            config::resolve_template(wd, &template_vars)?
        } else {
            worktree_path.to_string_lossy().into()
        };

        if verbose {
            eprintln!("Running teardown script: {}...", script.name);
        }

        let status = std::process::Command::new("sh")
            .arg("-c")
            .arg(&resolved_command)
            .current_dir(&working_dir)
            .status()
            .with_context(|| format!("failed to run teardown script '{}'", script.name))?;

        if !status.success() {
            eprintln!(
                "Warning: teardown script '{}' failed (exit code {}), continuing...",
                script.name,
                status.code().unwrap_or(-1)
            );
        }
    }

    // On Windows, directories can't be deleted while a process has them as cwd
    if cfg!(windows) {
        let cwd = std::env::current_dir().unwrap_or_default();
        if cwd.starts_with(worktree_path) {
            anyhow::bail!(
                "on Windows, you cannot clean up a workspace from inside its worktree \
                 (the directory is locked by your shell). Close this tab first, then retry."
            );
        }
    }

    // Remove worktree
    if verbose {
        eprintln!("Removing worktree...");
    }
    if let Err(first_err) = git::remove_worktree(source_path, worktree_path, false) {
        if cfg!(windows) && !tab_id.is_empty() {
            if verbose {
                eprintln!("Worktree directory is locked, closing terminal panes and retrying...");
            }
            if let Ok(backend) = terminal::detect_terminal() {
                let _ = backend.close_tab(tab_id);
            }
            std::thread::sleep(std::time::Duration::from_millis(1500));

            if worktree_path.exists() {
                std::fs::remove_dir_all(worktree_path).with_context(|| {
                    format!(
                        "could not remove worktree directory after closing panes. \
                         Close any processes using '{}' and retry.",
                        worktree_path.display()
                    )
                })?;
            }
        } else {
            return Err(first_err);
        }
    }

    // Handle local branch cleanup
    match branch_cleanup {
        BranchCleanup::Archive => {
            let has_commits = git::branch_has_commits(
                source_path,
                branch,
                &git::detect_main_branch(source_path)?,
            )
            .unwrap_or(true);
            if has_commits {
                if verbose {
                    eprintln!("Archiving branch '{branch}'...");
                }
                git::archive_branch(source_path, branch, &config.archive_prefix)?;
            } else {
                if verbose {
                    eprintln!("Deleting branch '{branch}' (no commits)...");
                }
                git::delete_branch(source_path, branch)?;
            }
        }
        BranchCleanup::Delete => {
            if verbose {
                eprintln!("Deleting local branch '{branch}'...");
            }
            // Branch may already be gone (e.g., worktree removal cleaned it up).
            let _ = git::delete_branch(source_path, branch);
        }
        BranchCleanup::None => {}
    }

    // Record history
    let _ = crate::history::record(history_event);

    // Update state
    state.remove(project_name, name);
    state.save_to(state_path)?;
    agent_hooks::remove_status(project_name, name);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_branch_name_with_prefix() {
        assert_eq!(
            compute_branch_name("my-feature", Some("user")),
            "user/my-feature"
        );
    }

    #[test]
    fn compute_branch_name_without_prefix() {
        assert_eq!(compute_branch_name("my-feature", None), "my-feature");
    }

    #[test]
    fn compute_branch_name_with_empty_prefix() {
        assert_eq!(compute_branch_name("my-feature", Some("")), "my-feature");
    }

    #[test]
    fn allocate_ports_contiguous_block() {
        let names = vec!["VITE_PORT".into(), "API_PORT".into(), "DB_PORT".into()];
        let ports = allocate_ports(&names, &[], 10000);
        assert_eq!(ports["VITE_PORT"], 10000);
        assert_eq!(ports["API_PORT"], 10001);
        assert_eq!(ports["DB_PORT"], 10002);
    }

    #[test]
    fn allocate_ports_skips_reserved() {
        let names = vec!["PORT_A".into()];
        let ports = allocate_ports(&names, &[10000], 10000);
        assert_eq!(ports["PORT_A"], 10001);
    }

    #[test]
    fn allocate_ports_finds_gap_after_reserved_block() {
        let names = vec!["PORT_A".into(), "PORT_B".into()];
        let ports = allocate_ports(&names, &[10000], 10000);
        assert_eq!(ports["PORT_A"], 10001);
        assert_eq!(ports["PORT_B"], 10002);
    }

    #[test]
    fn allocate_ports_skips_fragmented_reserved() {
        let names = vec!["PORT_A".into(), "PORT_B".into(), "PORT_C".into()];
        let reserved = vec![10000, 10002];
        let ports = allocate_ports(&names, &reserved, 10000);
        // Can't fit 3 contiguous starting at 10000 (taken), or 10001 (10002 taken)
        assert_eq!(ports["PORT_A"], 10003);
        assert_eq!(ports["PORT_B"], 10004);
        assert_eq!(ports["PORT_C"], 10005);
    }

    #[test]
    fn allocate_ports_empty_names() {
        let ports = allocate_ports(&[], &[], 10000);
        assert!(ports.is_empty());
    }

    #[test]
    fn allocate_ports_custom_range_start() {
        let names = vec!["PORT_A".into()];
        let ports = allocate_ports(&names, &[], 20000);
        assert_eq!(ports["PORT_A"], 20000);
    }
}
