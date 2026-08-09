use anyhow::{Context, Result};
use chrono::Utc;
use std::collections::HashSet;

use crate::agent_hooks;
use crate::config::{self, ResolvedConfig, TemplateVars};
use crate::git;
use crate::history;
use crate::state::Workspace;
use crate::terminal;

pub fn run(
    ctx: &mut super::WorkflowCtx,
    name: &str,
    prompt: Option<&str>,
    fetch: bool,
    issue_ref: Option<&str>,
    plan: bool,
) -> Result<()> {
    let (project_name, source_path, config, state_path, verbose) = (
        ctx.project,
        ctx.source_path,
        ctx.config,
        ctx.state_path,
        ctx.verbose,
    );
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
            ctx,
            name,
            &worktree_path,
            &HashSet::new(),
            prompt,
            &std::collections::HashMap::new(),
            plan,
        );
    }

    // Verify that all configured agents are installed before doing any work.
    // This prevents creating a worktree only to fail when launching the agent.
    for pane in &config.panes {
        if let Some(ref agent) = pane.agent {
            config::check_agent_available(agent)?;
        }
    }

    // Same reason: `open_workspace` checks this too, but that runs after the
    // branch and worktree exist, so a broken layout would leave them behind.
    config::validate_panes(&config.panes)?;

    // Clear stale conversation history if this worktree name was used before.
    // This prevents --continue from resuming a conversation from a previous
    // workspace that happened to have the same name.
    agent_hooks::clear_agent_conversations(&worktree_path);

    // Fetch and fast-forward main if requested (via --fetch flag or auto_fetch config)
    if fetch || config.auto_fetch {
        let remote = &config.fetch_remote;
        let main_branch = git::detect_main_branch(source_path)?;

        if verbose {
            eprintln!("Fetching from {remote}...");
        }
        git::fetch(source_path, remote)
            .with_context(|| format!("failed to fetch from remote '{remote}'"))?;

        if verbose {
            eprintln!("Fast-forwarding {main_branch} to {remote}/{main_branch}...");
        }
        git::ff_to_remote(source_path, remote, &main_branch).with_context(|| {
            format!(
                "could not fast-forward '{main_branch}' to '{remote}/{main_branch}'. \
                 Your local {main_branch} may have diverged from the remote. \
                 Resolve manually with `git pull` in your source repo, then retry."
            )
        })?;
    }

    if verbose {
        eprintln!("Creating branch '{branch}'...");
    }
    git::create_branch(source_path, &branch)
        .with_context(|| format!("failed to create branch '{branch}'"))?;

    // Record history event
    let _ = history::record(&history::HistoryEvent::started(
        project_name,
        name,
        &branch,
        issue_ref,
    ));

    if let Some(parent) = worktree_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }

    if verbose {
        eprintln!("Creating worktree at {}...", worktree_path.display());
    }
    git::create_worktree(source_path, &worktree_path, &branch)
        .with_context(|| "failed to create worktree")?;

    // Allocate ports for this workspace
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

    // Record state BEFORE setup scripts so discard can clean up on failure
    ctx.state.add(Workspace {
        project: project_name.into(),
        name: name.into(),
        branch: branch.clone(),
        worktree_path: worktree_path.to_string_lossy().into(),
        source_path: source_path.to_string_lossy().into(),
        created_at: Utc::now(),
        terminal_tab_id: String::new(),
        allocated_ports,
        pr_number: None,
        pr_url: None,
    });
    ctx.state.save_to(state_path)?;

    // Validate that no agent type appears in more than one pane
    let mut seen_agents = std::collections::HashMap::new();
    for pane in &config.panes {
        if let Some(ref agent) = pane.agent {
            if let Some(existing_pane) = seen_agents.get(agent) {
                anyhow::bail!(
                    "agent '{agent}' is configured in both pane '{existing_pane}' and pane '{}'. \
                     Each agent type can only appear in one pane per workspace.",
                    pane.name
                );
            }
            seen_agents.insert(agent.clone(), pane.name.clone());
        }
    }

    // Set up agent-specific workspace configuration for each unique agent in the panes
    let agents: Vec<String> = seen_agents.into_keys().collect();

    // Build worktree context for agents that support it (e.g., Claude SessionStart hook)
    let agent_context = build_agent_context(config, ctx.state, &worktree_path);

    for agent in &agents {
        if let Err(e) = agent_hooks::setup_agent_hooks(
            &worktree_path,
            source_path,
            project_name,
            name,
            agent,
            Some(&agent_context),
        ) && verbose
        {
            eprintln!("Warning: failed to set up agent hooks for {agent}: {e}");
        }
    }

    let template_vars = TemplateVars {
        source: source_path.to_string_lossy().into(),
        worktree: worktree_path.to_string_lossy().into(),
        branch: branch.clone(),
        name: name.into(),
        project: project_name.into(),
    };

    // Split scripts into immediate (blocking) and deferred (run in terminal pane)
    let (immediate, deferred_scripts): (Vec<_>, Vec<_>) =
        config.setup_scripts.iter().partition(|s| !s.deferred);

    // Run immediate scripts before opening the workspace, with the workspace's
    // allocated ports exported so a setup script binds what the panes will use.
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

    // Resolve deferred setup script commands
    let mut deferred_setup_commands = Vec::new();
    for script in &deferred_scripts {
        let resolved_command = config::resolve_template(&script.command, &template_vars)
            .with_context(|| format!("failed to resolve template in script '{}'", script.name))?;

        if verbose {
            eprintln!("Deferring setup script: {}...", script.name);
        }

        deferred_setup_commands.push(resolved_command);
    }

    // Where the deferred work goes is a property of the config, not of the
    // terminal — only its *delivery* differs by backend.
    let deferred = plan_deferred(config, &deferred_setup_commands, &template_vars)?;

    // Multiplexer backends (tmux, Zellij) block inside `open_workspace`, so
    // there is no "after" in which to send anything: their chain has to be
    // baked into the pane command upfront. Backends that return can have the
    // target pane's own command suppressed and the whole chain sent once the
    // workspace is up.
    let backend = terminal::detect_terminal()?;
    let can_defer = backend.supports_run_in_pane();

    let mut skip_command_panes: HashSet<String> = HashSet::new();
    let mut deferred_commands: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    if let Some(ref d) = deferred {
        if can_defer {
            skip_command_panes.insert(d.pane_name.clone());
        } else {
            deferred_commands.insert(d.pane_name.clone(), d.chain.clone());
        }
    }

    super::open::open_workspace(
        ctx,
        name,
        &worktree_path,
        &skip_command_panes,
        prompt,
        &deferred_commands,
        plan,
    )?;

    if can_defer && let Some(d) = deferred {
        let tab_id = ctx
            .state
            .find_by_worktree_path(&worktree_path.to_string_lossy())
            .map(|w| w.terminal_tab_id.clone())
            .unwrap_or_default();
        backend.run_in_pane(&tab_id, d.pane_index, &d.chain)?;
    }

    Ok(())
}

/// Which pane the deferred setup scripts run in, and what runs there.
///
/// Both delivery paths need the same two answers and used to work them out
/// separately, with predicates that had drifted apart. The `run_in_pane` path
/// matched *any* pane with `deferred = true`, including one running an agent —
/// which the suppression list deliberately excluded. So the agent launched at
/// open time and the setup scripts were then typed into the pane on top of it,
/// straight into the agent's prompt. Deciding once removes the possibility.
struct DeferredPlan {
    pane_index: usize,
    pane_name: String,
    /// The deferred setup scripts, then the pane's own command, joined by `&&`.
    chain: String,
}

fn plan_deferred(
    config: &ResolvedConfig,
    setup_commands: &[String],
    vars: &TemplateVars,
) -> Result<Option<DeferredPlan>> {
    // A pane running an agent belongs to that agent — anything chained into it
    // is typed at the agent rather than at a shell.
    for pane in config.panes.iter().filter(|p| p.deferred) {
        if pane.agent.is_some() {
            eprintln!(
                "Warning: pane '{}' sets `deferred` but runs agent '{}'. \
                 Deferred setup scripts cannot run in an agent pane, so this is ignored.",
                pane.name,
                pane.agent.as_deref().unwrap_or_default()
            );
        }
    }

    let targets: Vec<(usize, &config::PaneConfig)> = config
        .panes
        .iter()
        .enumerate()
        .filter(|(_, p)| p.deferred && p.agent.is_none())
        .collect();

    if targets.len() > 1 {
        eprintln!(
            "Warning: {} panes are marked `deferred`; only '{}' receives the deferred \
             setup scripts. The others start normally.",
            targets.len(),
            targets[0].1.name
        );
    }

    if let Some(&(pane_index, pane)) = targets.first() {
        let mut chain: Vec<String> = setup_commands.to_vec();
        if let Some(ref cmd) = pane.command {
            let resolved = config::resolve_template(cmd, vars)?;
            if !resolved.is_empty() {
                chain.push(resolved);
            }
        }
        if chain.is_empty() {
            return Ok(None);
        }
        return Ok(Some(DeferredPlan {
            pane_index,
            pane_name: pane.name.clone(),
            chain: chain.join(" && "),
        }));
    }

    if setup_commands.is_empty() {
        return Ok(None);
    }

    // No pane opted in, so fall back to a plain shell pane. There may not be
    // one — a layout of nothing but agents and fixed commands — in which case
    // the scripts have nowhere safe to run. Say so rather than picking pane 0,
    // which is the agent pane in the default layout.
    match config
        .panes
        .iter()
        .enumerate()
        .find(|(_, p)| p.command.is_none() && p.agent.is_none())
    {
        Some((pane_index, pane)) => Ok(Some(DeferredPlan {
            pane_index,
            pane_name: pane.name.clone(),
            chain: setup_commands.join(" && "),
        })),
        None => {
            eprintln!(
                "Warning: {} deferred setup script(s) have no pane to run in — every pane \
                 runs an agent or its own command. Mark a pane `deferred` to give them one.",
                setup_commands.len()
            );
            Ok(None)
        }
    }
}

/// Build worktree context string for agent system prompt injection.
/// Includes isolation note, pane descriptions, allocated ports, and
/// any user-configured context from `.foundry.toml`.
fn build_agent_context(
    config: &config::ResolvedConfig,
    state: &crate::state::WorkspaceState,
    worktree_path: &std::path::Path,
) -> String {
    let mut parts = Vec::new();

    // Isolation note
    parts.push(
        "You are working in a Foundry-managed worktree — an isolated copy of the repository. \
         Changes here do not affect the main branch or other workspaces. \
         Git push is intentionally blocked — the user handles pushing and PR creation externally."
            .to_string(),
    );

    // Pane descriptions
    let pane_lines: Vec<String> = config
        .panes
        .iter()
        .map(|pane| {
            if let Some(ref agent) = pane.agent {
                format!("- \"{}\": {} (agent)", pane.name, agent)
            } else if let Some(ref cmd) = pane.command {
                format!("- \"{}\": {}", pane.name, cmd)
            } else {
                format!("- \"{}\": shell", pane.name)
            }
        })
        .collect();
    if !pane_lines.is_empty() {
        parts.push(format!(
            "Workspace panes started by the user:\n{}",
            pane_lines.join("\n")
        ));
    }

    // Allocated ports
    if let Some(ws) = state.find_by_worktree_path(&worktree_path.to_string_lossy())
        && !ws.allocated_ports.is_empty()
    {
        let mut sorted: Vec<_> = ws.allocated_ports.iter().collect();
        sorted.sort_by_key(|(_, v)| *v);
        let port_lines: Vec<String> = sorted
            .iter()
            .map(|(name, port)| format!("- {name}: {port}"))
            .collect();
        parts.push(format!("Allocated ports:\n{}", port_lines.join("\n")));
    }

    // User-configured context from .foundry.toml
    if let Some(ref user_context) = config.context {
        let expanded =
            if let Some(ws) = state.find_by_worktree_path(&worktree_path.to_string_lossy()) {
                let mut ctx = user_context.clone();
                for (port_name, port_value) in &ws.allocated_ports {
                    ctx = ctx.replace(&format!("{{{port_name}}}"), &port_value.to_string());
                }
                ctx
            } else {
                user_context.clone()
            };
        if !expanded.trim().is_empty() {
            parts.push(expanded);
        }
    }

    parts.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{GlobalConfig, PaneConfig, SplitDirection, merge_configs};

    fn pane(name: &str, agent: Option<&str>, command: Option<&str>, deferred: bool) -> PaneConfig {
        PaneConfig {
            name: name.into(),
            agent: agent.map(Into::into),
            command: command.map(Into::into),
            split_from: (name != "agent").then(|| "agent".to_string()),
            direction: (name != "agent").then_some(SplitDirection::Right),
            optional: false,
            env: Default::default(),
            deferred,
        }
    }

    fn config_with(panes: Vec<PaneConfig>) -> ResolvedConfig {
        merge_configs(
            &GlobalConfig {
                panes,
                ..Default::default()
            },
            None,
        )
    }

    fn vars() -> TemplateVars {
        TemplateVars {
            source: "/src".into(),
            worktree: "/wt".into(),
            branch: "feat".into(),
            name: "feat".into(),
            project: "proj".into(),
        }
    }

    #[test]
    fn deferred_pane_receives_the_scripts_then_its_own_command() {
        let config = config_with(vec![
            pane("agent", Some("claude"), None, false),
            pane("dev", None, Some("pnpm dev"), true),
        ]);
        let plan = plan_deferred(&config, &["pnpm install".into()], &vars())
            .unwrap()
            .expect("expected a plan");
        assert_eq!(plan.pane_index, 1);
        assert_eq!(plan.pane_name, "dev");
        assert_eq!(plan.chain, "pnpm install && pnpm dev");
    }

    #[test]
    fn without_a_deferred_pane_the_scripts_go_to_the_shell_pane() {
        let config = config_with(vec![
            pane("agent", Some("claude"), None, false),
            pane("shell", None, None, false),
        ]);
        let plan = plan_deferred(&config, &["pnpm install".into()], &vars())
            .unwrap()
            .expect("expected a plan");
        assert_eq!(plan.pane_name, "shell");
        assert_eq!(plan.chain, "pnpm install");
    }

    /// The regression: the `run_in_pane` path matched any pane with
    /// `deferred = true`, so a deferred *agent* pane became the target. Its
    /// command was not suppressed, so the agent launched and the setup scripts
    /// were then typed into its prompt.
    #[test]
    fn a_deferred_agent_pane_is_never_the_target() {
        let config = config_with(vec![
            pane("agent", Some("claude"), None, true),
            pane("shell", None, None, false),
        ]);
        let plan = plan_deferred(&config, &["pnpm install".into()], &vars())
            .unwrap()
            .expect("expected a plan");
        assert_eq!(plan.pane_name, "shell", "must not target the agent pane");
        assert_eq!(plan.pane_index, 1);
    }

    /// Falling back to pane 0 would be the agent pane in the default layout.
    #[test]
    fn scripts_with_nowhere_safe_to_run_are_reported_not_forced_into_pane_zero() {
        let config = config_with(vec![
            pane("agent", Some("claude"), None, false),
            pane("logs", None, Some("tail -f log"), false),
        ]);
        assert!(
            plan_deferred(&config, &["pnpm install".into()], &vars())
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn nothing_deferred_means_no_plan() {
        let config = config_with(vec![
            pane("agent", Some("claude"), None, false),
            pane("shell", None, None, false),
        ]);
        assert!(plan_deferred(&config, &[], &vars()).unwrap().is_none());
    }

    /// A deferred pane with no scripts still defers its own command, so it
    /// starts after the blocking setup phase rather than during it.
    #[test]
    fn a_deferred_pane_with_no_scripts_still_defers_its_command() {
        let config = config_with(vec![
            pane("agent", Some("claude"), None, false),
            pane("dev", None, Some("pnpm dev"), true),
        ]);
        let plan = plan_deferred(&config, &[], &vars()).unwrap().unwrap();
        assert_eq!(plan.chain, "pnpm dev");
    }
}
