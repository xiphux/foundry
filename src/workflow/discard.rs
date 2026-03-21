use anyhow::{Context, Result};
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;

use crate::config::{self, ResolvedConfig, TemplateVars};
use crate::git;
use crate::state::WorkspaceState;
use crate::terminal;

#[allow(clippy::too_many_arguments)]
pub fn run(
    name: &str,
    project_name: &str,
    source_path: &Path,
    config: &ResolvedConfig,
    state: &mut WorkspaceState,
    state_path: &Path,
    verbose: bool,
    skip_confirm: bool,
) -> Result<()> {
    let worktree_path = config.worktree_dir.join(project_name).join(name);

    if !worktree_path.exists() {
        anyhow::bail!("worktree '{name}' does not exist at {}", worktree_path.display());
    }

    let workspace = state
        .find_by_worktree_path(&worktree_path.to_string_lossy())
        .ok_or_else(|| anyhow::anyhow!("workspace '{name}' not found in state"))?;
    let branch = workspace.branch.clone();
    let tab_id = workspace.terminal_tab_id.clone();

    if git::has_uncommitted_changes(&worktree_path)? && !skip_confirm {
        print!("Worktree has uncommitted changes. Discard anyway? [y/N] ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            eprintln!("Aborted.");
            return Ok(());
        }
    }

    if !tab_id.is_empty() {
        if verbose {
            eprintln!("Closing terminal tab...");
        }
        if let Ok(backend) = terminal::detect_terminal() {
            let _ = backend.close_tab(&tab_id);
        }
    }

    let template_vars = TemplateVars {
        source: source_path.to_string_lossy().into(),
        worktree: worktree_path.to_string_lossy().into(),
        branch: branch.clone(),
        name: name.into(),
        project: project_name.into(),
        agent_command: config.agent_command.clone(),
    };

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

        let status = Command::new("sh")
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

    if verbose {
        eprintln!("Removing worktree...");
    }
    git::remove_worktree(source_path, &worktree_path, true)?;

    if verbose {
        eprintln!("Archiving branch '{branch}'...");
    }
    git::archive_branch(source_path, &branch, &config.archive_prefix)?;

    state.remove(project_name, name);
    state.save_to(state_path)?;

    eprintln!("Discarded workspace '{name}'. Branch '{branch}' archived.");

    Ok(())
}
