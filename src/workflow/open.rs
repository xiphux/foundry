use anyhow::Result;
use std::collections::HashSet;
use std::path::Path;

use crate::agent_hooks;
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
    deferred_commands: &std::collections::HashMap<String, String>,
) -> Result<()> {
    let backend = terminal::detect_terminal()?;

    // If the workspace already has an open tab, just focus it instead of opening a duplicate
    let workspace = state.find_by_worktree_path(&worktree_path.to_string_lossy());
    if let Some(ws) = workspace
        && !ws.terminal_tab_id.is_empty()
        && let Ok(true) = backend.focus_tab(&ws.terminal_tab_id)
    {
        if verbose {
            eprintln!("Workspace '{name}' is already open, switching to it.");
        }
        return Ok(());
    }

    // Build template vars for pane commands from workspace state
    let workspace = state.find_by_worktree_path(&worktree_path.to_string_lossy());
    let source_path = workspace.map(|w| w.source_path.clone()).unwrap_or_default();
    let branch = workspace.map(|w| w.branch.clone()).unwrap_or_default();

    let template_vars = TemplateVars {
        source: source_path,
        worktree: worktree_path.to_string_lossy().into(),
        branch,
        name: name.into(),
        project: project_name.into(),
    };

    // Build PaneSpecs from the resolved config, resolving template variables.
    // Only the first agent pane receives the prompt — multiple agents acting on
    // the same prompt simultaneously would interfere with each other.
    let mut pane_specs = Vec::new();
    let mut prompt_assigned = false;
    for pane in &config.panes {
        let resolved_command = if let Some(deferred_cmd) = deferred_commands.get(&pane.name) {
            // Pre-chained deferred command (for backends that don't support run_in_pane)
            Some(deferred_cmd.clone())
        } else if skip_command_panes.contains(&pane.name) {
            // Command will be sent separately (e.g., after deferred setup scripts)
            None
        } else if let Some(ref agent) = pane.agent {
            // Pane has an agent — auto-generate the command.
            // Only the first agent pane gets the prompt.
            let pane_prompt = if !prompt_assigned {
                prompt_assigned = true;
                prompt
            } else {
                None
            };
            // Resume previous conversation if the agent supports it,
            // a conversation exists, and no new prompt is given.
            let continue_session = pane_prompt.is_none()
                && config::agent_capabilities(agent)
                    .and_then(|c| c.resume_flag)
                    .is_some()
                && agent_hooks::has_agent_conversation(agent, worktree_path);
            Some(config::build_agent_command(
                agent,
                config.custom_agent_command.as_deref(),
                pane_prompt,
                continue_session,
            ))
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

        // Merge allocated port env vars into this pane's env
        let mut env = pane.env.clone();
        if let Some(ws) = state.find_by_worktree_path(&worktree_path.to_string_lossy()) {
            for (port_name, port_value) in &ws.allocated_ports {
                env.insert(port_name.clone(), port_value.to_string());
            }
        }

        pane_specs.push(PaneSpec {
            name: pane.name.clone(),
            split_from: pane.split_from.clone(),
            direction: pane.direction.clone(),
            command: resolved_command,
            env,
            shell: config.shell.clone(),
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
