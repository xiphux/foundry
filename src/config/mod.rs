mod global;
mod project;
pub mod types;

pub use global::GlobalConfig;
pub use project::ProjectConfig;
pub use types::{MergeStrategy, PaneConfig, PaneOverride, ScriptConfig, SplitDirection};

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Resolved configuration after merging global + project configs.
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub branch_prefix: Option<String>,
    /// The default agent identifier ("claude", "codex", "every-code", "custom").
    /// Used for the default pane layout when no panes have explicit agent fields.
    pub agent: String,
    /// Custom agent command (only used when agent = "custom")
    pub custom_agent_command: Option<String>,
    pub archive_prefix: String,
    pub merge_strategy: MergeStrategy,
    pub worktree_dir: PathBuf,
    /// Whether to fetch and fast-forward main before branching
    pub auto_fetch: bool,
    /// Remote name to fetch from (default: "origin")
    pub fetch_remote: String,
    /// Remote name for PR operations (None = auto-detect)
    pub pr_remote: Option<String>,
    pub panes: Vec<PaneConfig>,
    pub setup_scripts: Vec<ScriptConfig>,
    pub teardown_scripts: Vec<ScriptConfig>,
    /// Custom prompt template for GitHub issues (None = use default)
    pub issue_prompt: Option<String>,
    /// Shell executable to use in terminal panes (e.g., "bash", "powershell").
    pub shell: Option<String>,
    /// Starting port for dynamic port allocation (default: 10000)
    pub port_range_start: u16,
    /// Named port slots to allocate per workspace (env var names)
    pub ports: Vec<String>,
}

/// Per-agent capabilities. Groups all agent-specific knowledge in one place
/// so adding a new agent is a single registry entry rather than touching
/// multiple match arms across the codebase.
pub struct AgentCapabilities {
    /// The CLI executable (and any default flags) for this agent.
    pub executable: &'static str,
    /// Flag to resume a previous conversation (e.g., "--continue", "--resume").
    pub resume_flag: Option<&'static str>,
    /// Whether the agent accepts the prompt as a trailing positional argument.
    pub prompt_is_positional: bool,
    /// Alternative executable names that should trigger the "use agent field" warning
    /// (e.g., "coder" for every-code). The agent identifier itself is always included.
    pub executable_aliases: &'static [&'static str],
}

/// Registry of known agents and their capabilities.
const AGENT_REGISTRY: &[(&str, AgentCapabilities)] = &[
    (
        "claude",
        AgentCapabilities {
            executable: "claude",
            resume_flag: Some("--continue"),
            prompt_is_positional: true,
            executable_aliases: &[],
        },
    ),
    (
        "codex",
        AgentCapabilities {
            executable: "codex --full-auto",
            resume_flag: Some("--resume"),
            prompt_is_positional: true,
            executable_aliases: &[],
        },
    ),
    (
        "every-code",
        AgentCapabilities {
            executable: "coder --full-auto",
            resume_flag: Some("--resume"),
            prompt_is_positional: true,
            executable_aliases: &["coder"],
        },
    ),
];

/// Look up capabilities for a known agent. Returns None for unknown/custom agents.
pub fn agent_capabilities(agent: &str) -> Option<&'static AgentCapabilities> {
    AGENT_REGISTRY
        .iter()
        .find(|(id, _)| *id == agent)
        .map(|(_, caps)| caps)
}

/// Build the full agent command string for a given agent identifier,
/// optionally including a prompt and/or session resume flag.
/// For known agents, the prompt is passed as a positional argument.
/// If `continue_session` is true, adds the appropriate flag to resume
/// the previous conversation.
pub fn build_agent_command(
    agent: &str,
    custom_command: Option<&str>,
    prompt: Option<&str>,
    continue_session: bool,
) -> String {
    let base = resolve_agent_command(agent, custom_command);
    let caps = agent_capabilities(agent);

    let with_continue = if continue_session {
        if let Some(flag) = caps.and_then(|c| c.resume_flag) {
            format!("{base} {flag}")
        } else {
            base
        }
    } else {
        base
    };

    match prompt {
        Some(p) if !p.is_empty() => {
            if caps.is_some_and(|c| c.prompt_is_positional) {
                let escaped = p.replace('\'', "'\\''");
                format!("{with_continue} '{escaped}'")
            } else {
                with_continue
            }
        }
        _ => with_continue,
    }
}

/// Check if a pane command looks like it's invoking a known agent, and warn
/// that the `agent` field should be used instead.
pub fn warn_agent_in_command(panes: &[PaneConfig]) {
    for pane in panes {
        if pane.agent.is_some() {
            continue;
        }
        if let Some(ref cmd) = pane.command {
            let first_word = cmd.split_whitespace().next().unwrap_or("");
            // Check against all agent identifiers and their executable aliases
            for (agent_id, caps) in AGENT_REGISTRY {
                if first_word == *agent_id
                    || caps.executable.split_whitespace().next() == Some(first_word)
                    || caps.executable_aliases.contains(&first_word)
                {
                    eprintln!(
                        "Warning: pane '{}' uses command '{cmd}' which looks like a known agent. \
                         Use `agent = \"{agent_id}\"` instead of `command` so foundry can set up \
                         permissions, status tracking, and prompt passthrough.",
                        pane.name
                    );
                    break;
                }
            }
        }
    }
}

/// Resolve the base agent command from the agent identifier.
/// For known agents, returns the executable with any default flags.
pub fn resolve_agent_command(agent: &str, custom_command: Option<&str>) -> String {
    if agent == "custom" {
        return custom_command.unwrap_or("claude").to_string();
    }
    match agent_capabilities(agent) {
        Some(caps) => caps.executable.to_string(),
        None => agent.to_string(),
    }
}

/// Values available for template variable substitution.
#[derive(Debug, Clone)]
pub struct TemplateVars {
    pub source: String,
    pub worktree: String,
    pub branch: String,
    pub name: String,
    pub project: String,
}

/// The set of known template variable names.
const KNOWN_VARS: &[&str] = &["source", "worktree", "branch", "name", "project"];

/// Validate that a template string only uses known variable names.
/// Called at config parse time. Does NOT resolve values.
pub fn validate_template(template: &str) -> Result<()> {
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' {
            let var_name: String = chars.by_ref().take_while(|&c| c != '}').collect();
            if !KNOWN_VARS.contains(&var_name.as_str()) {
                anyhow::bail!("unknown template variable: {{{var_name}}}");
            }
        }
    }
    Ok(())
}

/// Resolve template variables in a string. Returns an error for unknown variables.
pub fn resolve_template(template: &str, vars: &TemplateVars) -> Result<String> {
    let mut result = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '{' {
            let var_name: String = chars.by_ref().take_while(|&c| c != '}').collect();
            let value = match var_name.as_str() {
                "source" => &vars.source,
                "worktree" => &vars.worktree,
                "branch" => &vars.branch,
                "name" => &vars.name,
                "project" => &vars.project,
                _ => anyhow::bail!("unknown template variable: {{{var_name}}}"),
            };
            result.push_str(value);
        } else {
            result.push(c);
        }
    }

    Ok(result)
}

/// Load the global config from ~/.foundry/config.toml.
/// Returns defaults if the file doesn't exist.
pub fn load_global_config() -> Result<GlobalConfig> {
    let config_dir = foundry_dir()?;
    let config_path = config_dir.join("config.toml");

    if !config_path.exists() {
        return Ok(GlobalConfig::default());
    }

    let contents = std::fs::read_to_string(&config_path)
        .with_context(|| format!("failed to read {}", config_path.display()))?;
    let config: GlobalConfig = toml::from_str(&contents)
        .with_context(|| format!("failed to parse {}", config_path.display()))?;

    // Validate template variables in pane commands at parse time
    for pane in &config.panes {
        if let Some(ref cmd) = pane.command {
            validate_template(cmd).with_context(|| format!("in pane '{}' command", pane.name))?;
        }
    }

    Ok(config)
}

/// Load the project config from .foundry.toml in the given repo root.
/// Returns None if the file doesn't exist.
pub fn load_project_config(repo_root: &Path) -> Result<Option<ProjectConfig>> {
    let config_path = repo_root.join(".foundry.toml");

    if !config_path.exists() {
        return Ok(None);
    }

    let contents = std::fs::read_to_string(&config_path)
        .with_context(|| format!("failed to read {}", config_path.display()))?;
    let config: ProjectConfig = toml::from_str(&contents)
        .with_context(|| format!("failed to parse {}", config_path.display()))?;

    // Validate template variables in scripts at parse time
    for script in &config.scripts.setup {
        validate_template(&script.command)
            .with_context(|| format!("in setup script '{}'", script.name))?;
        if let Some(ref wd) = script.working_dir {
            validate_template(wd)
                .with_context(|| format!("in setup script '{}' working_dir", script.name))?;
        }
    }
    for script in &config.scripts.teardown {
        validate_template(&script.command)
            .with_context(|| format!("in teardown script '{}'", script.name))?;
        if let Some(ref wd) = script.working_dir {
            validate_template(wd)
                .with_context(|| format!("in teardown script '{}' working_dir", script.name))?;
        }
    }

    Ok(Some(config))
}

/// Merge global and project configs into a resolved config.
pub fn merge_configs(global: &GlobalConfig, project: Option<&ProjectConfig>) -> ResolvedConfig {
    let worktree_dir_str = project
        .and_then(|p| p.worktree_dir.as_deref())
        .unwrap_or(&global.worktree_dir);

    let worktree_dir = expand_tilde(worktree_dir_str);

    let agent = project
        .and_then(|p| p.agent.clone())
        .unwrap_or_else(|| global.agent.clone());

    let custom_agent_command = project
        .and_then(|p| p.agent_command.clone())
        .or_else(|| global.agent_command.clone());

    let mut panes: Vec<PaneConfig> = global
        .panes
        .iter()
        .filter_map(|pane| {
            let project_override = project.and_then(|p| p.panes.get(&pane.name));

            if pane.optional && project_override.is_none() {
                return None;
            }

            let mut merged = pane.clone();
            if let Some(ov) = project_override {
                if let Some(ref a) = ov.agent {
                    merged.agent = Some(a.clone());
                }
                if let Some(ref cmd) = ov.command {
                    merged.command = Some(cmd.clone());
                }
                if !ov.env.is_empty() {
                    merged.env.extend(ov.env.clone());
                }
                if ov.deferred {
                    merged.deferred = true;
                }
            }
            Some(merged)
        })
        .collect();

    // If no pane has an explicit agent, apply the global agent to the first pane
    // that has no command (the default "agent" pane in the default layout).
    let has_any_agent_pane = panes.iter().any(|p| p.agent.is_some());
    if !has_any_agent_pane
        && let Some(first) = panes.first_mut()
        && first.command.is_none()
    {
        first.agent = Some(agent.clone());
    }

    let resolved = ResolvedConfig {
        branch_prefix: project
            .and_then(|p| p.branch_prefix.clone())
            .or_else(|| global.branch_prefix.clone()),
        agent,
        custom_agent_command,
        archive_prefix: project
            .and_then(|p| p.archive_prefix.clone())
            .unwrap_or_else(|| global.archive_prefix.clone()),
        merge_strategy: project
            .and_then(|p| p.merge_strategy.clone())
            .unwrap_or_else(|| global.merge_strategy.clone()),
        worktree_dir,
        auto_fetch: project
            .and_then(|p| p.auto_fetch)
            .unwrap_or(global.auto_fetch),
        fetch_remote: project
            .and_then(|p| p.fetch_remote.clone())
            .or_else(|| global.fetch_remote.clone())
            .unwrap_or_else(|| "origin".into()),
        pr_remote: project
            .and_then(|p| p.pr_remote.clone())
            .or_else(|| global.pr_remote.clone()),
        panes,
        setup_scripts: project.map(|p| p.scripts.setup.clone()).unwrap_or_default(),
        teardown_scripts: project
            .map(|p| p.scripts.teardown.clone())
            .unwrap_or_default(),
        issue_prompt: global.issue_prompt.clone(),
        shell: project
            .and_then(|p| p.shell.clone())
            .or_else(|| global.shell.clone()),
        port_range_start: global.port_range_start.unwrap_or(10000),
        ports: project.map(|p| p.ports.clone()).unwrap_or_default(),
    };

    warn_agent_in_command(&resolved.panes);

    resolved
}

/// Expand ~ to home directory.
pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    PathBuf::from(path)
}

/// Get the foundry base directory (~/.foundry/).
pub fn foundry_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("could not determine home directory")?;
    Ok(home.join(".foundry"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_tilde_expands_home_dir() {
        let result = expand_tilde("~/projects");
        let expected = dirs::home_dir().unwrap().join("projects");
        assert_eq!(result, expected);
    }

    #[test]
    fn expand_tilde_no_tilde_passthrough() {
        let result = expand_tilde("/absolute/path");
        assert_eq!(result, PathBuf::from("/absolute/path"));
    }

    #[test]
    fn expand_tilde_in_middle_of_path() {
        let result = expand_tilde("/some/~/path");
        assert_eq!(result, PathBuf::from("/some/~/path"));
    }

    #[test]
    fn resolve_agent_command_claude() {
        assert_eq!(resolve_agent_command("claude", None), "claude");
    }

    #[test]
    fn resolve_agent_command_codex() {
        let cmd = resolve_agent_command("codex", None);
        assert!(cmd.starts_with("codex "));
        assert!(cmd.contains("--full-auto"));
    }

    #[test]
    fn resolve_agent_command_every_code() {
        let cmd = resolve_agent_command("every-code", None);
        assert!(cmd.starts_with("coder "));
        assert!(cmd.contains("--full-auto"));
    }

    #[test]
    fn resolve_agent_command_custom_with_command() {
        assert_eq!(
            resolve_agent_command("custom", Some("my-agent --flag")),
            "my-agent --flag"
        );
    }

    #[test]
    fn resolve_agent_command_custom_without_command_defaults_to_claude() {
        assert_eq!(resolve_agent_command("custom", None), "claude");
    }

    #[test]
    fn resolve_agent_command_unknown_passthrough() {
        assert_eq!(
            resolve_agent_command("some-other-agent", None),
            "some-other-agent"
        );
    }

    #[test]
    fn validate_template_empty_string() {
        assert!(validate_template("").is_ok());
    }

    #[test]
    fn validate_template_no_variables() {
        assert!(validate_template("echo hello world").is_ok());
    }

    #[test]
    fn validate_template_valid_variable() {
        assert!(validate_template("cd {worktree} && ls").is_ok());
    }

    #[test]
    fn validate_template_invalid_variable_name() {
        let result = validate_template("cd {nonexistent}");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("unknown template variable")
        );
    }

    #[test]
    fn validate_template_unclosed_brace_with_unknown_var() {
        // The parser reads chars until '}', so an unclosed brace consumes
        // the rest of the string as the variable name. If the resulting
        // string is not a known variable, it returns an error.
        let result = validate_template("cd {bogus");
        assert!(result.is_err());
    }

    #[test]
    fn validate_template_unclosed_brace_with_known_var_succeeds() {
        // An unclosed brace around a known variable name still succeeds
        // because the parser treats the remaining text as the var name.
        assert!(validate_template("cd {worktree").is_ok());
    }

    #[test]
    fn agent_registry_has_known_agents() {
        assert!(agent_capabilities("claude").is_some());
        assert!(agent_capabilities("codex").is_some());
        assert!(agent_capabilities("every-code").is_some());
        assert!(agent_capabilities("unknown").is_none());
    }

    #[test]
    fn agent_capabilities_claude() {
        let caps = agent_capabilities("claude").unwrap();
        assert_eq!(caps.executable, "claude");
        assert_eq!(caps.resume_flag, Some("--continue"));
        assert!(caps.prompt_is_positional);
    }

    #[test]
    fn agent_capabilities_codex() {
        let caps = agent_capabilities("codex").unwrap();
        assert!(caps.executable.contains("--full-auto"));
        assert_eq!(caps.resume_flag, Some("--resume"));
    }

    #[test]
    fn warn_agent_in_command_detects_agent_commands() {
        // This test verifies the detection logic (warning goes to stderr,
        // which we can't easily capture, but we verify the function doesn't panic)
        let panes = vec![PaneConfig {
            name: "test".into(),
            agent: None,
            command: Some("claude --dangerously-skip-permissions".into()),
            split_from: None,
            direction: None,
            optional: false,
            env: Default::default(),
            deferred: false,
        }];
        warn_agent_in_command(&panes); // should warn but not panic
    }

    #[test]
    fn warn_agent_in_command_skips_agent_panes() {
        // Panes with agent set should not trigger warnings even if command is also set
        let panes = vec![PaneConfig {
            name: "test".into(),
            agent: Some("claude".into()),
            command: Some("claude".into()),
            split_from: None,
            direction: None,
            optional: false,
            env: Default::default(),
            deferred: false,
        }];
        warn_agent_in_command(&panes); // should not warn
    }
}
