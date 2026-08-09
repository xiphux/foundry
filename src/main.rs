mod cli;

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser};
use cli::Cli;
use foundry::config::{self, ResolvedConfig};
use foundry::registry::Registry;
use foundry::state::WorkspaceState;
use foundry::workflow;
use std::path::{Path, PathBuf};

/// Resolved workspace identity: name, project, and source repo path.
struct WorkspaceRef {
    name: String,
    project_name: String,
    source_path: PathBuf,
}

/// Resolve a workspace by explicit name (via registry) or by inferring from cwd (via state).
fn resolve_workspace(
    explicit_name: Option<String>,
    project: Option<&str>,
    registry_path: &Path,
    state: &WorkspaceState,
    command: &str,
) -> Result<WorkspaceRef> {
    match explicit_name {
        Some(name) => {
            workflow::validate_workspace_name(&name)?;
            let mut registry = Registry::load_from(registry_path)?;
            let (project_name, source_path) =
                workflow::resolve_project(project, &mut registry, registry_path)?;
            Ok(WorkspaceRef {
                name,
                project_name,
                source_path,
            })
        }
        None => {
            let cwd = std::env::current_dir()?;
            let cwd_str = cwd.to_string_lossy();
            let ws = state.find_by_worktree_path(&cwd_str).ok_or_else(|| {
                anyhow::anyhow!(
                    "could not infer workspace from current directory. \
                     Provide a name: `foundry {command} <name>`"
                )
            })?;
            Ok(WorkspaceRef {
                name: ws.name.clone(),
                project_name: ws.project.clone(),
                source_path: PathBuf::from(&ws.source_path),
            })
        }
    }
}

/// Load and merge global + project configs.
fn load_config(source_path: &Path) -> Result<ResolvedConfig> {
    let global_config = config::load_global_config()?;
    let project_config = config::load_project_config(source_path)?;
    Ok(config::merge_configs(
        &global_config,
        project_config.as_ref(),
    ))
}

/// Restore the default `SIGPIPE` disposition.
///
/// Rust ignores `SIGPIPE` and surfaces the closed pipe as an `EPIPE` error,
/// which `println!` turns into a panic — so `foundry diff | head` printed a
/// "failed printing to stdout: Broken pipe" backtrace instead of just stopping.
/// Restoring the default makes foundry exit quietly the way every other Unix
/// tool in a pipeline does.
#[cfg(unix)]
fn restore_sigpipe() {
    // SAFETY: called once at startup, before any threads are spawned, and only
    // resets a signal to the disposition the process would have had without
    // Rust's runtime overriding it.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
fn restore_sigpipe() {}

/// Build the context every workflow runs inside.
fn ctx<'a>(
    project: &'a str,
    source_path: &'a Path,
    config: &'a ResolvedConfig,
    state: &'a mut WorkspaceState,
    state_path: &'a Path,
    verbose: bool,
) -> workflow::WorkflowCtx<'a> {
    workflow::WorkflowCtx {
        project,
        source_path,
        config,
        state,
        state_path,
        verbose,
    }
}

fn main() -> Result<()> {
    restore_sigpipe();

    let cli = Cli::parse();

    let (registry_path, state_path) = workflow::foundry_paths()?;

    match cli.command {
        cli::Commands::Start {
            name,
            issue,
            prompt,
            prompt_file,
            fetch,
            plan,
            agent,
        } => {
            let mut registry = Registry::load_from(&registry_path)?;
            let (project_name, source_path) =
                workflow::resolve_project(cli.project.as_deref(), &mut registry, &registry_path)?;
            let mut resolved = load_config(&source_path)?;

            // Override the primary agent if --agent is specified
            if let Some(ref agent_override) = agent {
                resolved.override_primary_agent(agent_override);
            }

            let mut state = WorkspaceState::load_from(&state_path)?;

            // Resolve name and prompt from --issue, --prompt, or --prompt-file
            let (resolved_name, prompt_text) = if let Some(ref issue_ref) = issue {
                let gh_issue = foundry::github::fetch_issue(issue_ref)?;
                let auto_name =
                    name.unwrap_or_else(|| foundry::github::issue_to_worktree_name(&gh_issue));
                // Use issue as prompt unless --prompt or --prompt-file explicitly provided
                let issue_prompt = if prompt.is_some() || prompt_file.is_some() {
                    None
                } else {
                    Some(foundry::github::issue_to_prompt(
                        &gh_issue,
                        resolved.issue_prompt.as_deref(),
                    ))
                };
                (auto_name, issue_prompt)
            } else {
                let n = name.ok_or_else(|| {
                    anyhow::anyhow!("a name is required. Usage: foundry start <name> or foundry start --issue <number>")
                })?;
                (n, None)
            };

            // --prompt or --prompt-file override the issue prompt
            let prompt_text =
                if let Some(p) = prompt {
                    Some(p)
                } else if let Some(ref path) = prompt_file {
                    Some(std::fs::read_to_string(path).with_context(|| {
                        format!("failed to read prompt file: {}", path.display())
                    })?)
                } else {
                    prompt_text
                };

            workflow::validate_workspace_name(&resolved_name)?;

            let mut c = ctx(
                &project_name,
                &source_path,
                &resolved,
                &mut state,
                &state_path,
                cli.verbose,
            );
            workflow::start::run(
                &mut c,
                &resolved_name,
                prompt_text.as_deref(),
                fetch,
                issue.as_deref(),
                plan,
            )?;
        }
        cli::Commands::Open { name, all } => {
            let mut state = WorkspaceState::load_from(&state_path)?;
            state.prune_stale();

            if all {
                let mut registry = Registry::load_from(&registry_path)?;
                let (project_name, source_path) = workflow::resolve_project(
                    cli.project.as_deref(),
                    &mut registry,
                    &registry_path,
                )?;
                let resolved = load_config(&source_path)?;

                // Carry the recorded worktree path, not just the name: rebuilding
                // it from `worktree_dir` skips every workspace created before
                // that setting was last changed.
                let workspaces: Vec<(String, PathBuf)> = state
                    .find_by_project(&project_name)
                    .iter()
                    .map(|w| (w.name.clone(), PathBuf::from(&w.worktree_path)))
                    .collect();

                if workspaces.is_empty() {
                    println!("No active workspaces for project '{project_name}'.");
                } else {
                    // Only backends whose open can return before the tab is
                    // ready (Ghostty, iTerm2, Windows Terminal) need a pause
                    // between opens; the rest block until the workspace is up
                    // and were sleeping for nothing. Detection is memoized, so
                    // this is cheap.
                    let settle = foundry::terminal::detect_terminal()?.settle_delay();

                    for (i, (ws_name, worktree_path)) in workspaces.iter().enumerate() {
                        if !worktree_path.exists() {
                            eprintln!("Warning: worktree '{ws_name}' no longer exists, skipping.");
                            continue;
                        }

                        if cli.verbose {
                            eprintln!("Opening workspace '{ws_name}'...");
                        }

                        let mut c = ctx(
                            &project_name,
                            &source_path,
                            &resolved,
                            &mut state,
                            &state_path,
                            cli.verbose,
                        );
                        workflow::open::open_workspace(
                            &mut c,
                            ws_name,
                            worktree_path,
                            &std::collections::HashSet::new(),
                            None,
                            &std::collections::HashMap::new(),
                            false,
                        )?;

                        // Brief pause between opens to let the terminal settle
                        if i < workspaces.len() - 1 && !settle.is_zero() {
                            std::thread::sleep(settle);
                        }
                    }
                    eprintln!("Opened {} workspace(s).", workspaces.len());
                }
            } else if let Some(name) = name {
                workflow::validate_workspace_name(&name)?;
                let mut registry = Registry::load_from(&registry_path)?;
                let (project_name, source_path) = workflow::resolve_project(
                    cli.project.as_deref(),
                    &mut registry,
                    &registry_path,
                )?;
                let resolved = load_config(&source_path)?;

                // Prefer the recorded path — rebuilding it from `worktree_dir`
                // misses workspaces created before that setting last changed.
                // Fall back to the derived path so a worktree that exists on
                // disk but is absent from state can still be opened.
                let worktree_path = state
                    .find(&project_name, &name)
                    .map(|w| PathBuf::from(&w.worktree_path))
                    .unwrap_or_else(|| resolved.worktree_dir.join(&project_name).join(&name));
                if !worktree_path.exists() {
                    anyhow::bail!(
                        "worktree '{name}' does not exist at {}. \
                         Use `foundry start {name}` to create it.",
                        worktree_path.display()
                    );
                }

                let mut c = ctx(
                    &project_name,
                    &source_path,
                    &resolved,
                    &mut state,
                    &state_path,
                    cli.verbose,
                );
                workflow::open::open_workspace(
                    &mut c,
                    &name,
                    &worktree_path,
                    &std::collections::HashSet::new(),
                    None, // no prompt for open
                    &std::collections::HashMap::new(),
                    false,
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
        cli::Commands::Edit { name } => {
            let mut state = WorkspaceState::load_from(&state_path)?;
            let ws =
                resolve_workspace(name, cli.project.as_deref(), &registry_path, &state, "edit")?;
            let resolved = load_config(&ws.source_path)?;
            let mut c = ctx(
                &ws.project_name,
                &ws.source_path,
                &resolved,
                &mut state,
                &state_path,
                cli.verbose,
            );
            workflow::edit::run(&mut c, &ws.name)?;
        }
        cli::Commands::Browse { name } => {
            let mut state = WorkspaceState::load_from(&state_path)?;
            let ws = resolve_workspace(
                name,
                cli.project.as_deref(),
                &registry_path,
                &state,
                "browse",
            )?;
            let resolved = load_config(&ws.source_path)?;
            let mut c = ctx(
                &ws.project_name,
                &ws.source_path,
                &resolved,
                &mut state,
                &state_path,
                cli.verbose,
            );
            workflow::edit::browse(&mut c, &ws.name)?;
        }
        cli::Commands::Diff { name, stat } => {
            let mut state = WorkspaceState::load_from(&state_path)?;
            let ws =
                resolve_workspace(name, cli.project.as_deref(), &registry_path, &state, "diff")?;
            let resolved = load_config(&ws.source_path)?;
            let mut c = ctx(
                &ws.project_name,
                &ws.source_path,
                &resolved,
                &mut state,
                &state_path,
                cli.verbose,
            );
            workflow::diff::run(&mut c, &ws.name, stat)?;
        }
        cli::Commands::Switch { name } => {
            let mut state = WorkspaceState::load_from(&state_path)?;
            state.prune_stale();

            if let Some(name) = name {
                workflow::validate_workspace_name(&name)?;
                let mut registry = Registry::load_from(&registry_path)?;
                let (project_name, source_path) = workflow::resolve_project(
                    cli.project.as_deref(),
                    &mut registry,
                    &registry_path,
                )?;
                // The trust gate runs here; nothing else is needed from the config.
                let _ = load_config(&source_path)?;

                // Look the workspace up by identity rather than by a path
                // rebuilt from `worktree_dir`, which stopped matching whenever
                // that setting changed and left `switch` unable to find a tab
                // for a workspace that was open in front of the user. The
                // fallback is the recorded path, which is the tab id the bare
                // backend hands out.
                let tab_id = state
                    .find(&project_name, &name)
                    .map(|w| {
                        if w.terminal_tab_id.is_empty() {
                            w.worktree_path.clone()
                        } else {
                            w.terminal_tab_id.clone()
                        }
                    })
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "workspace '{name}' is not active in project '{project_name}'. \
                             Run `foundry list` to see active workspaces."
                        )
                    })?;

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
        cli::Commands::Finish { name, local } => {
            let mut state = WorkspaceState::load_from(&state_path)?;
            let ws = resolve_workspace(
                name,
                cli.project.as_deref(),
                &registry_path,
                &state,
                "finish",
            )?;
            let resolved = load_config(&ws.source_path)?;

            let mut c = ctx(
                &ws.project_name,
                &ws.source_path,
                &resolved,
                &mut state,
                &state_path,
                cli.verbose,
            );
            workflow::finish::run(&mut c, &ws.name, local, cli.yes)?;
        }
        cli::Commands::Discard { name, force } => {
            let mut state = WorkspaceState::load_from(&state_path)?;
            let ws = resolve_workspace(
                name,
                cli.project.as_deref(),
                &registry_path,
                &state,
                "discard",
            )?;
            let resolved = load_config(&ws.source_path)?;

            let mut c = ctx(
                &ws.project_name,
                &ws.source_path,
                &resolved,
                &mut state,
                &state_path,
                cli.verbose,
            );
            workflow::discard::run(&mut c, &ws.name, cli.yes, force)?;
        }
        cli::Commands::Restore { branch } => {
            let mut registry = Registry::load_from(&registry_path)?;
            let (project_name, source_path) =
                workflow::resolve_project(cli.project.as_deref(), &mut registry, &registry_path)?;
            let resolved = load_config(&source_path)?;

            if let Some(branch) = branch {
                let mut state = WorkspaceState::load_from(&state_path)?;
                let mut c = ctx(
                    &project_name,
                    &source_path,
                    &resolved,
                    &mut state,
                    &state_path,
                    cli.verbose,
                );
                workflow::restore::run(&mut c, &branch)?;
            } else {
                workflow::restore::list_archived(&source_path, &resolved.archive_prefix)?;
            }
        }
        cli::Commands::Trust { path, revoke } => {
            let store_path = foundry::trust::trust_store_path()?;
            let mut store = foundry::trust::TrustStore::load_from(&store_path)?;

            // Revoking only removes a map key, so it must not require a live
            // repository — the case that most needs it is an entry left behind
            // by a repo that has since been deleted or moved.
            if revoke {
                let target = match path {
                    Some(p) => p,
                    None => std::env::current_dir()?,
                };
                if store.revoke(&target) {
                    store.save_to(&store_path)?;
                    println!("Withdrew approval for {}.", target.display());
                } else {
                    println!("{} was not approved.", target.display());
                }
                return Ok(());
            }

            // Locate the config from the *local* toplevel, so that inside a
            // linked worktree this reads and hashes the same `.foundry.toml`
            // the gate will. Making the two agree on which project the
            // approval belongs to is `trust::key_for`'s job, not this one —
            // resolving the main repo here instead would show the user one
            // file and approve the hash of another.
            let repo_root = match path {
                Some(p) => foundry::git::repo_root(&p)
                    .with_context(|| format!("{} is not inside a git repository", p.display()))?,
                None => {
                    let cwd = std::env::current_dir()?;
                    foundry::git::repo_root(&cwd).context("not inside a git repository")?
                }
            };

            let config_path = repo_root.join(".foundry.toml");
            if !config_path.exists() {
                anyhow::bail!("no .foundry.toml at {}", config_path.display());
            }
            let contents = std::fs::read_to_string(&config_path)
                .with_context(|| format!("failed to read {}", config_path.display()))?;
            let project_config: config::ProjectConfig = toml::from_str(&contents)
                .with_context(|| format!("failed to parse {}", config_path.display()))?;

            let directives = foundry::trust::executable_directives(&project_config);
            if directives.is_empty() {
                println!(
                    "{} contains no commands — no approval needed.",
                    config_path.display()
                );
                return Ok(());
            }

            // Same sanitizing as the interactive prompt: this command prints
            // the directives and then approves without asking, so its display
            // is the last thing seen before the approval is persisted.
            println!("{} will run:", config_path.display());
            for directive in foundry::trust::render_directives(&directives) {
                println!("  - {directive}");
            }

            store.trust(&repo_root, &foundry::trust::hash_config(&contents));
            store.save_to(&store_path)?;
            println!("\nApproved. Foundry will ask again if the file changes.");
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
        cli::Commands::History { limit } => {
            foundry::history::display(limit)?;
        }
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
        cli::Commands::Status { watch } => {
            let mut state = WorkspaceState::load_from(&state_path)?;
            state.prune_stale();
            state.save_to(&state_path)?;
            workflow::status::run(&state, &state_path, watch)?;
        }
        cli::Commands::Pr { name, title, body } => {
            let mut state = WorkspaceState::load_from(&state_path)?;
            let ws = resolve_workspace(name, cli.project.as_deref(), &registry_path, &state, "pr")?;
            let resolved = load_config(&ws.source_path)?;

            let mut c = ctx(
                &ws.project_name,
                &ws.source_path,
                &resolved,
                &mut state,
                &state_path,
                cli.verbose,
            );
            workflow::pr::run(&mut c, &ws.name, title.as_deref(), body.as_deref())?;
        }
        cli::Commands::Checks { name } => {
            let mut state = WorkspaceState::load_from(&state_path)?;
            let ws = resolve_workspace(
                name,
                cli.project.as_deref(),
                &registry_path,
                &state,
                "checks",
            )?;
            let resolved = load_config(&ws.source_path)?;

            let mut c = ctx(
                &ws.project_name,
                &ws.source_path,
                &resolved,
                &mut state,
                &state_path,
                cli.verbose,
            );
            workflow::checks::run(&mut c, &ws.name)?;
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
