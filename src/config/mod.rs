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
    pub panes: Vec<PaneConfig>,
    pub setup_scripts: Vec<ScriptConfig>,
    pub teardown_scripts: Vec<ScriptConfig>,
    /// Custom prompt template for GitHub issues (None = use default)
    pub issue_prompt: Option<String>,
}

/// Build the full agent command string for a given agent identifier,
/// optionally including a prompt. For known agents (claude, codex, every-code),
/// the prompt is passed as a positional argument.
pub fn build_agent_command(
    agent: &str,
    custom_command: Option<&str>,
    prompt: Option<&str>,
) -> String {
    let base = resolve_agent_command(agent, custom_command);

    match prompt {
        Some(p) if !p.is_empty() => {
            let escaped = p.replace('\'', "'\\''");
            match agent {
                "claude" | "codex" | "every-code" => format!("{base} '{escaped}'"),
                _ => base,
            }
        }
        _ => base,
    }
}

/// Known agent executable names. Used to warn when users put agent commands
/// in the `command` field instead of using the `agent` field.
const KNOWN_AGENT_EXECUTABLES: &[&str] = &["claude", "codex", "coder"];

/// Check if a pane command looks like it's invoking a known agent, and warn
/// that the `agent` field should be used instead.
pub fn warn_agent_in_command(panes: &[PaneConfig]) {
    for pane in panes {
        if pane.agent.is_some() {
            continue;
        }
        if let Some(ref cmd) = pane.command {
            let first_word = cmd.split_whitespace().next().unwrap_or("");
            if KNOWN_AGENT_EXECUTABLES.contains(&first_word) {
                eprintln!(
                    "Warning: pane '{}' uses command '{cmd}' which looks like a known agent. \
                     Use `agent = \"{first_word}\"` instead of `command` so foundry can set up \
                     permissions, status tracking, and prompt passthrough.",
                    pane.name
                );
            }
        }
    }
}

/// Resolve the base agent command from the agent identifier.
/// For codex/every-code, includes CLI flags for autonomous operation in worktrees.
pub fn resolve_agent_command(agent: &str, custom_command: Option<&str>) -> String {
    match agent {
        "claude" => "claude".to_string(),
        "codex" => "codex --full-auto".to_string(),
        "every-code" => "coder --full-auto".to_string(),
        "custom" => custom_command.unwrap_or("claude").to_string(),
        other => other.to_string(),
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
    if !has_any_agent_pane {
        if let Some(first) = panes.first_mut() {
            if first.command.is_none() {
                first.agent = Some(agent.clone());
            }
        }
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
        panes,
        setup_scripts: project.map(|p| p.scripts.setup.clone()).unwrap_or_default(),
        teardown_scripts: project
            .map(|p| p.scripts.teardown.clone())
            .unwrap_or_default(),
        issue_prompt: global.issue_prompt.clone(),
    };

    warn_agent_in_command(&resolved.panes);

    resolved
}

/// Expand ~ to home directory.
pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
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
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("unknown template variable"));
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
    fn known_agent_executables_detected() {
        // Should match: first word is a known agent executable
        assert!(KNOWN_AGENT_EXECUTABLES.contains(&"claude"));
        assert!(KNOWN_AGENT_EXECUTABLES.contains(&"codex"));
        assert!(KNOWN_AGENT_EXECUTABLES.contains(&"coder"));
        // Should not match
        assert!(!KNOWN_AGENT_EXECUTABLES.contains(&"npm"));
        assert!(!KNOWN_AGENT_EXECUTABLES.contains(&"claude-helper"));
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
