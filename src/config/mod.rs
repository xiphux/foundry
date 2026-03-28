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
    /// All executable names this agent might be invoked as in a pane command.
    /// Used by `warn_agent_in_command` to detect when users should use the
    /// `agent` field instead of `command`.
    pub names: &'static [&'static str],
    /// Build the full command string for this agent.
    /// Each agent knows its own executable, flags, and how to incorporate
    /// the prompt and resume parameters.
    pub build_command: fn(prompt: Option<&str>, resume: bool) -> String,
}

/// Escape a prompt string for use in a shell single-quoted argument.
fn escape_prompt(prompt: &str) -> String {
    prompt.replace('\'', "'\\''")
}

/// Registry of known agents and their capabilities.
const AGENT_REGISTRY: &[(&str, AgentCapabilities)] = &[
    (
        "claude",
        AgentCapabilities {
            names: &["claude"],
            build_command: |prompt, resume| {
                let mut cmd = "claude".to_string();
                if resume {
                    cmd += " --continue";
                }
                if let Some(p) = prompt {
                    cmd += &format!(" '{}'", escape_prompt(p));
                }
                cmd
            },
        },
    ),
    (
        "codex",
        AgentCapabilities {
            names: &["codex"],
            build_command: |prompt, resume| {
                let mut cmd = "codex --full-auto".to_string();
                if resume {
                    cmd += " --resume";
                }
                if let Some(p) = prompt {
                    cmd += &format!(" '{}'", escape_prompt(p));
                }
                cmd
            },
        },
    ),
    (
        "every-code",
        AgentCapabilities {
            names: &["coder", "every-code"],
            build_command: |prompt, resume| {
                let mut cmd = "coder --full-auto".to_string();
                if resume {
                    cmd += " --resume";
                }
                if let Some(p) = prompt {
                    cmd += &format!(" '{}'", escape_prompt(p));
                }
                cmd
            },
        },
    ),
    (
        "gemini",
        AgentCapabilities {
            names: &["gemini"],
            build_command: |prompt, resume| {
                let mut cmd = "gemini -y".to_string();
                if resume {
                    cmd += " --resume";
                }
                if let Some(p) = prompt {
                    cmd += &format!(" -p '{}'", escape_prompt(p));
                }
                cmd
            },
        },
    ),
    (
        "aider",
        AgentCapabilities {
            names: &["aider"],
            // Aider is an interactive REPL. We launch with --yes for auto-approve
            // but don't pass --message (which would auto-exit after processing).
            // The user interacts with the REPL directly.
            build_command: |_prompt, _resume| "aider --yes".to_string(),
        },
    ),
    (
        "copilot",
        AgentCapabilities {
            names: &["copilot"],
            build_command: |prompt, _resume| {
                let mut cmd = "copilot --yolo".to_string();
                if let Some(p) = prompt {
                    cmd += &format!(" -p '{}'", escape_prompt(p));
                }
                cmd
            },
        },
    ),
    (
        "kiro",
        AgentCapabilities {
            names: &["kiro", "kiro-cli"],
            build_command: |prompt, resume| {
                let mut cmd = "kiro-cli chat --trust-all-tools".to_string();
                if resume {
                    cmd += " --resume";
                }
                if let Some(p) = prompt {
                    cmd += &format!(" '{}'", escape_prompt(p));
                }
                cmd
            },
        },
    ),
    (
        "opencode",
        AgentCapabilities {
            names: &["opencode"],
            build_command: |prompt, resume| {
                let mut cmd = "opencode --yolo".to_string();
                if resume {
                    cmd += " --continue";
                }
                if let Some(p) = prompt {
                    cmd += &format!(" -p '{}'", escape_prompt(p));
                }
                cmd
            },
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
/// Each agent's `build_command` knows how to construct its own CLI invocation.
pub fn build_agent_command(
    agent: &str,
    custom_command: Option<&str>,
    prompt: Option<&str>,
    continue_session: bool,
) -> String {
    if agent == "custom" {
        return custom_command.unwrap_or("claude").to_string();
    }
    let non_empty_prompt = prompt.filter(|p| !p.is_empty());
    match agent_capabilities(agent) {
        Some(caps) => (caps.build_command)(non_empty_prompt, continue_session),
        None => agent.to_string(),
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
            for (agent_id, caps) in AGENT_REGISTRY {
                if first_word == *agent_id || caps.names.contains(&first_word) {
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
/// For known agents, returns the command without prompt or resume flags.
pub fn resolve_agent_command(agent: &str, custom_command: Option<&str>) -> String {
    build_agent_command(agent, custom_command, None, false)
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
        assert!(agent_capabilities("gemini").is_some());
        assert!(agent_capabilities("aider").is_some());
        assert!(agent_capabilities("copilot").is_some());
        assert!(agent_capabilities("kiro").is_some());
        assert!(agent_capabilities("opencode").is_some());
        assert!(agent_capabilities("unknown").is_none());
    }

    #[test]
    fn agent_build_command_claude() {
        let caps = agent_capabilities("claude").unwrap();
        assert_eq!((caps.build_command)(None, false), "claude");
        assert_eq!((caps.build_command)(None, true), "claude --continue");
        assert_eq!(
            (caps.build_command)(Some("fix the bug"), false),
            "claude 'fix the bug'"
        );
        assert_eq!(
            (caps.build_command)(Some("fix the bug"), true),
            "claude --continue 'fix the bug'"
        );
    }

    #[test]
    fn agent_build_command_codex() {
        let caps = agent_capabilities("codex").unwrap();
        let cmd = (caps.build_command)(None, false);
        assert!(cmd.starts_with("codex "));
        assert!(cmd.contains("--full-auto"));
        let cmd_resume = (caps.build_command)(None, true);
        assert!(cmd_resume.contains("--resume"));
    }

    #[test]
    fn agent_build_command_every_code() {
        let caps = agent_capabilities("every-code").unwrap();
        let cmd = (caps.build_command)(None, false);
        assert!(cmd.starts_with("coder "));
        assert!(cmd.contains("--full-auto"));
    }

    #[test]
    fn agent_build_command_gemini() {
        let caps = agent_capabilities("gemini").unwrap();
        assert_eq!((caps.build_command)(None, false), "gemini -y");
        assert_eq!((caps.build_command)(None, true), "gemini -y --resume");
        assert_eq!(
            (caps.build_command)(Some("fix the bug"), false),
            "gemini -y -p 'fix the bug'"
        );
    }

    #[test]
    fn agent_build_command_aider() {
        let caps = agent_capabilities("aider").unwrap();
        // Aider is always interactive — prompt and resume are ignored
        assert_eq!((caps.build_command)(None, false), "aider --yes");
        assert_eq!((caps.build_command)(Some("fix it"), true), "aider --yes");
    }

    #[test]
    fn agent_build_command_copilot() {
        let caps = agent_capabilities("copilot").unwrap();
        assert_eq!((caps.build_command)(None, false), "copilot --yolo");
        assert_eq!(
            (caps.build_command)(Some("fix the bug"), false),
            "copilot --yolo -p 'fix the bug'"
        );
        // No resume support — resume flag is ignored
        assert_eq!((caps.build_command)(None, true), "copilot --yolo");
    }

    #[test]
    fn agent_build_command_kiro() {
        let caps = agent_capabilities("kiro").unwrap();
        assert_eq!(
            (caps.build_command)(None, false),
            "kiro-cli chat --trust-all-tools"
        );
        assert_eq!(
            (caps.build_command)(None, true),
            "kiro-cli chat --trust-all-tools --resume"
        );
        assert_eq!(
            (caps.build_command)(Some("fix the bug"), false),
            "kiro-cli chat --trust-all-tools 'fix the bug'"
        );
    }

    #[test]
    fn agent_build_command_opencode() {
        let caps = agent_capabilities("opencode").unwrap();
        assert_eq!((caps.build_command)(None, false), "opencode --yolo");
        assert_eq!(
            (caps.build_command)(None, true),
            "opencode --yolo --continue"
        );
        assert_eq!(
            (caps.build_command)(Some("fix the bug"), false),
            "opencode --yolo -p 'fix the bug'"
        );
    }

    #[test]
    fn agent_names_detection() {
        let caps = agent_capabilities("claude").unwrap();
        assert!(caps.names.contains(&"claude"));

        let caps = agent_capabilities("every-code").unwrap();
        assert!(caps.names.contains(&"coder"));
        assert!(caps.names.contains(&"every-code"));
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
