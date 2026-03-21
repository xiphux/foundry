use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

use crate::config::{self, ResolvedConfig, TemplateVars};
use crate::state::WorkspaceState;
use crate::terminal;

pub fn open_workspace(
    project_name: &str,
    name: &str,
    worktree_path: &Path,
    config: &ResolvedConfig,
    state: &mut WorkspaceState,
    state_path: &Path,
    verbose: bool,
) -> Result<()> {
    let backend = terminal::detect_terminal()?;

    let workspace = state
        .find_by_worktree_path(&worktree_path.to_string_lossy());
    let source_path = workspace.map(|w| w.source_path.clone()).unwrap_or_default();
    let branch = workspace.map(|w| w.branch.clone()).unwrap_or_default();

    let template_vars = TemplateVars {
        source: source_path,
        worktree: worktree_path.to_string_lossy().into(),
        branch,
        name: name.into(),
        project: project_name.into(),
        agent_command: config.agent_command.clone(),
    };

    let mut pane_handles: HashMap<String, String> = HashMap::new();

    if config.panes.is_empty() {
        backend.open_tab(worktree_path)?;
        return Ok(());
    }

    let first = &config.panes[0];
    if verbose {
        eprintln!("Opening tab for pane '{}'...", first.name);
    }
    let handle = backend.open_tab(worktree_path)?;

    if let Some(ref cmd) = first.command {
        let resolved = config::resolve_template(cmd, &template_vars)?;
        if !resolved.is_empty() {
            backend.run_command(&handle, &resolved, &first.env)?;
        }
    }
    pane_handles.insert(first.name.clone(), handle.clone());

    for pane in &config.panes[1..] {
        let split_from = pane
            .split_from
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("pane '{}' has no split_from", pane.name))?;

        let parent_handle = pane_handles
            .get(split_from)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "pane '{}' references unknown split_from '{}'",
                    pane.name,
                    split_from
                )
            })?;

        let direction = pane
            .direction
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("pane '{}' has no direction", pane.name))?;

        if verbose {
            eprintln!("Splitting pane '{}'...", pane.name);
        }

        let new_handle = backend.split_pane(parent_handle, direction)?;

        if let Some(ref cmd) = pane.command {
            let resolved = config::resolve_template(cmd, &template_vars)?;
            if !resolved.is_empty() {
                backend.run_command(&new_handle, &resolved, &pane.env)?;
            }
        }

        pane_handles.insert(pane.name.clone(), new_handle);
    }

    state.set_terminal_tab_id(project_name, name, handle);
    state.save_to(state_path)?;

    Ok(())
}

pub fn list_workspaces(state: &WorkspaceState, project: &str) {
    let workspaces = state.find_by_project(project);
    if workspaces.is_empty() {
        println!("No active workspaces for project '{project}'.");
        return;
    }
    println!("Active workspaces for '{project}':");
    for ws in workspaces {
        println!("  {} (branch: {}, path: {})", ws.name, ws.branch, ws.worktree_path);
    }
}
