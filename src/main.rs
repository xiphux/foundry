mod cli;

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser};
use cli::Cli;
use foundry::config;
use foundry::registry::Registry;
use foundry::state::WorkspaceState;
use foundry::workflow;

fn main() -> Result<()> {
    let cli = Cli::parse();

    let (registry_path, state_path) = workflow::foundry_paths()?;

    match cli.command {
        cli::Commands::Start {
            name,
            prompt,
            prompt_file,
        } => {
            let mut registry = Registry::load_from(&registry_path)?;
            let (project_name, source_path) =
                workflow::resolve_project(cli.project.as_deref(), &mut registry, &registry_path)?;

            let global_config = config::load_global_config()?;
            let project_config = config::load_project_config(&source_path)?;
            let resolved = config::merge_configs(&global_config, project_config.as_ref());

            let mut state = WorkspaceState::load_from(&state_path)?;

            // Resolve prompt from --prompt or --prompt-file
            let prompt_text =
                if let Some(p) = prompt {
                    Some(p)
                } else if let Some(ref path) = prompt_file {
                    Some(std::fs::read_to_string(path).with_context(|| {
                        format!("failed to read prompt file: {}", path.display())
                    })?)
                } else {
                    None
                };

            workflow::start::run(
                &name,
                &project_name,
                &source_path,
                &resolved,
                &mut state,
                &state_path,
                cli.verbose,
                prompt_text.as_deref(),
            )?;
        }
        cli::Commands::Open { name } => {
            let mut state = WorkspaceState::load_from(&state_path)?;
            state.prune_stale();

            if let Some(name) = name {
                let mut registry = Registry::load_from(&registry_path)?;
                let (project_name, source_path) = workflow::resolve_project(
                    cli.project.as_deref(),
                    &mut registry,
                    &registry_path,
                )?;
                let global_config = config::load_global_config()?;
                let project_config = config::load_project_config(&source_path)?;
                let resolved = config::merge_configs(&global_config, project_config.as_ref());

                let worktree_path = resolved.worktree_dir.join(&project_name).join(&name);
                if !worktree_path.exists() {
                    anyhow::bail!("worktree '{name}' does not exist. Use `foundry start {name}` to create it.");
                }

                workflow::open::open_workspace(
                    &project_name,
                    &name,
                    &worktree_path,
                    &resolved,
                    &mut state,
                    &state_path,
                    cli.verbose,
                    &std::collections::HashSet::new(),
                )?;
            } else {
                let mut registry = Registry::load_from(&registry_path)?;
                let (project_name, _) = workflow::resolve_project(
                    cli.project.as_deref(),
                    &mut registry,
                    &registry_path,
                )?;
                workflow::open::list_workspaces(&state, &project_name);
            }
        }
        cli::Commands::Switch { name } => {
            let mut state = WorkspaceState::load_from(&state_path)?;
            state.prune_stale();

            if let Some(name) = name {
                let mut registry = Registry::load_from(&registry_path)?;
                let (project_name, source_path) = workflow::resolve_project(
                    cli.project.as_deref(),
                    &mut registry,
                    &registry_path,
                )?;
                let global_config = config::load_global_config()?;
                let project_config = config::load_project_config(&source_path)?;
                let resolved = config::merge_configs(&global_config, project_config.as_ref());

                let worktree_path = resolved.worktree_dir.join(&project_name).join(&name);
                let tab_id = worktree_path.to_string_lossy().to_string();

                let backend = foundry::terminal::detect_terminal()?;
                if !backend.focus_tab(&tab_id)? {
                    anyhow::bail!(
                        "could not find terminal tab for workspace '{name}'. \
                         Use `foundry open {name}` to open it."
                    );
                }
            } else {
                let mut registry = Registry::load_from(&registry_path)?;
                let (project_name, _) = workflow::resolve_project(
                    cli.project.as_deref(),
                    &mut registry,
                    &registry_path,
                )?;
                workflow::open::list_workspaces(&state, &project_name);
            }
        }
        cli::Commands::Finish { name } => {
            let mut state = WorkspaceState::load_from(&state_path)?;

            // Resolve workspace name and project. When inferring from cwd,
            // use the workspace state to get the project (not git::repo_root,
            // which would point at the worktree instead of the source repo).
            let (name, project_name, source_path) = match name {
                Some(n) => {
                    let mut registry = Registry::load_from(&registry_path)?;
                    let (pn, sp) = workflow::resolve_project(
                        cli.project.as_deref(),
                        &mut registry,
                        &registry_path,
                    )?;
                    (n, pn, sp)
                }
                None => {
                    let cwd = std::env::current_dir()?;
                    let cwd_str = cwd.to_string_lossy();
                    let ws = state
                        .find_by_worktree_path(&cwd_str)
                        .ok_or_else(|| anyhow::anyhow!(
                            "could not infer workspace from current directory. Provide a name: `foundry finish <name>`"
                        ))?;
                    (
                        ws.name.clone(),
                        ws.project.clone(),
                        std::path::PathBuf::from(&ws.source_path),
                    )
                }
            };

            let global_config = config::load_global_config()?;
            let project_config = config::load_project_config(&source_path)?;
            let resolved = config::merge_configs(&global_config, project_config.as_ref());

            workflow::finish::run(
                &name,
                &project_name,
                &source_path,
                &resolved,
                &mut state,
                &state_path,
                cli.verbose,
            )?;
        }
        cli::Commands::Discard { name } => {
            let mut state = WorkspaceState::load_from(&state_path)?;

            let (name, project_name, source_path) = match name {
                Some(n) => {
                    let mut registry = Registry::load_from(&registry_path)?;
                    let (pn, sp) = workflow::resolve_project(
                        cli.project.as_deref(),
                        &mut registry,
                        &registry_path,
                    )?;
                    (n, pn, sp)
                }
                None => {
                    let cwd = std::env::current_dir()?;
                    let cwd_str = cwd.to_string_lossy();
                    let ws = state
                        .find_by_worktree_path(&cwd_str)
                        .ok_or_else(|| anyhow::anyhow!(
                            "could not infer workspace from current directory. Provide a name: `foundry discard <name>`"
                        ))?;
                    (
                        ws.name.clone(),
                        ws.project.clone(),
                        std::path::PathBuf::from(&ws.source_path),
                    )
                }
            };

            let global_config = config::load_global_config()?;
            let project_config = config::load_project_config(&source_path)?;
            let resolved = config::merge_configs(&global_config, project_config.as_ref());

            workflow::discard::run(
                &name,
                &project_name,
                &source_path,
                &resolved,
                &mut state,
                &state_path,
                cli.verbose,
                cli.yes,
            )?;
        }
        cli::Commands::Restore { branch } => {
            let mut registry = Registry::load_from(&registry_path)?;
            let (project_name, source_path) =
                workflow::resolve_project(cli.project.as_deref(), &mut registry, &registry_path)?;

            let global_config = config::load_global_config()?;
            let project_config = config::load_project_config(&source_path)?;
            let resolved = config::merge_configs(&global_config, project_config.as_ref());

            if let Some(branch) = branch {
                let mut state = WorkspaceState::load_from(&state_path)?;
                workflow::restore::run(
                    &branch,
                    &project_name,
                    &source_path,
                    &resolved,
                    &mut state,
                    &state_path,
                    cli.verbose,
                )?;
            } else {
                workflow::restore::list_archived(&source_path, &resolved.archive_prefix)?;
            }
        }
        cli::Commands::Projects(cmd) => match cmd {
            cli::ProjectsCommands::List => {
                let registry = Registry::load_from(&registry_path)?;
                let projects = registry.list();
                if projects.is_empty() {
                    println!("No registered projects.");
                } else {
                    for (name, path) in &projects {
                        println!("  {name}: {}", path.display());
                    }
                }
            }
            cli::ProjectsCommands::Add { name, path } => {
                let mut registry = Registry::load_from(&registry_path)?;
                let abs_path = std::fs::canonicalize(&path).unwrap_or(path);
                registry.add(&name, abs_path)?;
                registry.save_to(&registry_path)?;
                println!("Project '{name}' registered.");
            }
            cli::ProjectsCommands::Remove { name } => {
                let mut registry = Registry::load_from(&registry_path)?;
                let state = WorkspaceState::load_from(&state_path)?;
                let active = state.find_by_project(&name);
                if !active.is_empty() {
                    eprintln!(
                        "Warning: project '{name}' has {} active workspace(s). \
                         Finish or discard them first.",
                        active.len()
                    );
                }
                registry.remove(&name)?;
                registry.save_to(&registry_path)?;
                println!("Project '{name}' removed.");
            }
        },
        cli::Commands::List => {
            let mut state = WorkspaceState::load_from(&state_path)?;
            state.prune_stale();
            state.save_to(&state_path)?;
            let workspaces = state.list();
            if workspaces.is_empty() {
                println!("No active workspaces.");
            } else {
                for ws in workspaces {
                    println!(
                        "  {}/{} (branch: {}, path: {})",
                        ws.project, ws.name, ws.branch, ws.worktree_path
                    );
                }
            }
        }
        cli::Commands::Status => {
            let mut state = WorkspaceState::load_from(&state_path)?;
            state.prune_stale();
            state.save_to(&state_path)?;
            workflow::status::run(&state)?;
        }
        cli::Commands::Completions { shell } => {
            clap_complete::generate(
                shell,
                &mut Cli::command(),
                "foundry",
                &mut std::io::stdout(),
            );
        }
    }

    Ok(())
}
