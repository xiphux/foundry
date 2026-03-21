use anyhow::{Context, Result};
use chrono::Utc;
use std::path::Path;
use std::process::Command;

use crate::config::{self, ResolvedConfig, TemplateVars};
use crate::git;
use crate::state::{Workspace, WorkspaceState};
use crate::terminal;

pub fn run(
    name: &str,
    project_name: &str,
    source_path: &Path,
    config: &ResolvedConfig,
    state: &mut WorkspaceState,
    state_path: &Path,
    verbose: bool,
) -> Result<()> {
    let branch = super::compute_branch_name(name, config.branch_prefix.as_deref());
    let worktree_path = config.worktree_dir.join(project_name).join(name);

    if worktree_path.exists() {
        if verbose {
            eprintln!(
                "Worktree already exists at {}, opening workspace...",
                worktree_path.display()
            );
        }
        return super::open::open_workspace(
            project_name,
            name,
            &worktree_path,
            config,
            state,
            state_path,
            verbose,
        );
    }

    if verbose {
        eprintln!("Creating branch '{branch}'...");
    }
    git::create_branch(source_path, &branch)
        .with_context(|| format!("failed to create branch '{branch}'"))?;

    if let Some(parent) = worktree_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }

    if verbose {
        eprintln!("Creating worktree at {}...", worktree_path.display());
    }
    git::create_worktree(source_path, &worktree_path, &branch)
        .with_context(|| "failed to create worktree")?;

    // Record state BEFORE setup scripts so discard can clean up on failure
    state.add(Workspace {
        project: project_name.into(),
        name: name.into(),
        branch: branch.clone(),
        worktree_path: worktree_path.to_string_lossy().into(),
        source_path: source_path.to_string_lossy().into(),
        created_at: Utc::now(),
        terminal_tab_id: String::new(),
    });
    state.save_to(state_path)?;

    let template_vars = TemplateVars {
        source: source_path.to_string_lossy().into(),
        worktree: worktree_path.to_string_lossy().into(),
        branch: branch.clone(),
        name: name.into(),
        project: project_name.into(),
        agent_command: config.agent_command.clone(),
    };

    // Split scripts into immediate (blocking) and deferred (run in terminal pane)
    let (immediate, deferred): (Vec<_>, Vec<_>) =
        config.setup_scripts.iter().partition(|s| !s.deferred);

    // Run immediate scripts before opening the workspace
    for script in &immediate {
        let resolved_command = config::resolve_template(&script.command, &template_vars)
            .with_context(|| format!("failed to resolve template in script '{}'", script.name))?;

        let working_dir = if let Some(ref wd) = script.working_dir {
            config::resolve_template(wd, &template_vars)?
        } else {
            worktree_path.to_string_lossy().into()
        };

        if verbose {
            eprintln!("Running setup script: {}...", script.name);
        }

        let status = Command::new("sh")
            .arg("-c")
            .arg(&resolved_command)
            .current_dir(&working_dir)
            .status()
            .with_context(|| format!("failed to run setup script '{}'", script.name))?;

        if !status.success() {
            anyhow::bail!(
                "setup script '{}' failed with exit code {}. \
                 Worktree left in place at {}. \
                 Fix the issue and re-run `foundry start {name}`, or clean up with `foundry discard {name}`.",
                script.name,
                status.code().unwrap_or(-1),
                worktree_path.display()
            );
        }
    }

    // Open the workspace
    super::open::open_workspace(
        project_name,
        name,
        &worktree_path,
        config,
        state,
        state_path,
        verbose,
    )?;

    // Run deferred scripts in the shell pane (first pane with no command)
    if !deferred.is_empty() {
        let shell_pane_index = config
            .panes
            .iter()
            .position(|p| p.command.is_none())
            .unwrap_or(0);

        // Build a chained command for all deferred scripts
        let mut deferred_commands = Vec::new();
        for script in &deferred {
            let resolved_command = config::resolve_template(&script.command, &template_vars)
                .with_context(|| {
                    format!("failed to resolve template in script '{}'", script.name)
                })?;

            if verbose {
                eprintln!("Deferring setup script to shell pane: {}...", script.name);
            }

            deferred_commands.push(resolved_command);
        }

        let chained = deferred_commands.join(" && ");
        let tab_id = worktree_path.to_string_lossy().to_string();

        if let Ok(backend) = terminal::detect_terminal() {
            backend.run_in_pane(&tab_id, shell_pane_index, &chained)?;
        }
    }

    Ok(())
}
