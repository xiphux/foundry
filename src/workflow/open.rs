use anyhow::Result;
use std::collections::HashSet;
use std::path::Path;

use crate::config::{self, ResolvedConfig, TemplateVars};
use crate::state::WorkspaceState;
use crate::terminal::{self, PaneSpec};

/// Open the terminal workspace for an existing worktree.
/// `skip_command_panes` contains pane names whose commands should be suppressed
/// (the pane is still created, but no command is sent). Used by `start` to
/// defer certain pane commands until after setup scripts complete.
#[allow(clippy::too_many_arguments)]
pub fn open_workspace(
    project_name: &str,
    name: &str,
    worktree_path: &Path,
    config: &ResolvedConfig,
    state: &mut WorkspaceState,
    state_path: &Path,
    verbose: bool,
    skip_command_panes: &HashSet<String>,
    prompt: Option<&str>,
) -> Result<()> {
    let backend = terminal::detect_terminal()?;

    // Build template vars for pane commands from workspace state
    let workspace = state.find_by_worktree_path(&worktree_path.to_string_lossy());
    let source_path = workspace.map(|w| w.source_path.clone()).unwrap_or_default();
    let branch = workspace.map(|w| w.branch.clone()).unwrap_or_default();

    // Build agent command with prompt if provided
    let agent_command = config::build_agent_command(config, prompt);

    let template_vars = TemplateVars {
        source: source_path,
        worktree: worktree_path.to_string_lossy().into(),
        branch,
        name: name.into(),
        project: project_name.into(),
        agent_command,
    };

    // Build PaneSpecs from the resolved config, resolving template variables
    let mut pane_specs = Vec::new();
    for pane in &config.panes {
        let resolved_command = if skip_command_panes.contains(&pane.name) {
            // Command will be sent separately (e.g., after deferred setup scripts)
            None
        } else if let Some(ref cmd) = pane.command {
            let resolved = config::resolve_template(cmd, &template_vars)?;
            if resolved.is_empty() {
                None
            } else {
                Some(resolved)
            }
        } else {
            None
        };

        pane_specs.push(PaneSpec {
            name: pane.name.clone(),
            split_from: pane.split_from.clone(),
            direction: pane.direction.clone(),
            command: resolved_command,
            env: pane.env.clone(),
        });
    }

    // Open the workspace — the backend builds the entire layout in one shot
    let tab_id = backend.open_workspace(worktree_path, &pane_specs, verbose)?;

    // Persist tab ID in state for later close_tab
    state.set_terminal_tab_id(project_name, name, tab_id);
    state.save_to(state_path)?;

    Ok(())
}

/// List active worktrees for a project.
pub fn list_workspaces(state: &WorkspaceState, project: &str) {
    let workspaces = state.find_by_project(project);
    if workspaces.is_empty() {
        println!("No active workspaces for project '{project}'.");
        return;
    }
    println!("Active workspaces for '{project}':");
    for ws in workspaces {
        println!(
            "  {} (branch: {}, path: {})",
            ws.name, ws.branch, ws.worktree_path
        );
    }
}
