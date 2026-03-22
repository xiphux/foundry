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
    /// The known agent identifier ("claude", "codex", "coder", "custom")
    pub agent: String,
    /// The base agent command (derived from agent, or custom agent_command)
    pub agent_command: String,
    pub archive_prefix: String,
    pub merge_strategy: MergeStrategy,
    pub worktree_dir: PathBuf,
    pub panes: Vec<PaneConfig>,
    pub setup_scripts: Vec<ScriptConfig>,
    pub teardown_scripts: Vec<ScriptConfig>,
    /// Custom prompt template for GitHub issues (None = use default)
    pub issue_prompt: Option<String>,
}

/// Build the full agent command string, optionally including a prompt.
/// For known agents (claude, codex), the prompt is passed as a positional argument.
/// For custom agents, the prompt is not appended (the user's custom command is used as-is).
pub fn build_agent_command(config: &ResolvedConfig, prompt: Option<&str>) -> String {
    let base = &config.agent_command;

    match prompt {
        Some(p) if !p.is_empty() => {
            // Shell-escape the prompt for safe embedding in a command
            let escaped = p.replace('\'', "'\\''");
            match config.agent.as_str() {
                "claude" | "codex" | "coder" => format!("{base} '{escaped}'"),
                _ => base.clone(), // Custom agents: don't append prompt
            }
        }
        _ => base.clone(),
    }
}

/// Resolve the base agent command from the agent identifier.
/// For codex, includes CLI flags for autonomous operation in worktrees
/// (sandbox scoped to workspace, no approval prompts).
fn resolve_agent_command(agent: &str, custom_command: Option<&str>) -> String {
    match agent {
        "claude" => "claude".to_string(),
        "codex" => {
            "codex -c 'sandbox_mode=\"workspace-write\"' -c 'approval_policy=\"never\"'".to_string()
        }
        "coder" => "coder --full-auto".to_string(),
        "custom" => custom_command.unwrap_or("claude").to_string(),
        // If someone puts a command directly in agent (backwards compat),
        // use it as-is
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
    pub agent_command: String,
}

/// The set of known template variable names.
const KNOWN_VARS: &[&str] = &[
    "source",
    "worktree",
    "branch",
    "name",
    "project",
    "agent_command",
];

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
                "agent_command" => &vars.agent_command,
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

    let panes = global
        .panes
        .iter()
        .filter_map(|pane| {
            let project_override = project.and_then(|p| p.panes.get(&pane.name));

            if pane.optional && project_override.is_none() {
                return None;
            }

            let mut merged = pane.clone();
            if let Some(ov) = project_override {
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

    let agent = project
        .and_then(|p| p.agent.clone())
        .unwrap_or_else(|| global.agent.clone());

    let custom_command = project
        .and_then(|p| p.agent_command.clone())
        .or_else(|| global.agent_command.clone());

    let agent_command = resolve_agent_command(&agent, custom_command.as_deref());

    ResolvedConfig {
        branch_prefix: project
            .and_then(|p| p.branch_prefix.clone())
            .or_else(|| global.branch_prefix.clone()),
        agent,
        agent_command,
        archive_prefix: project
            .and_then(|p| p.archive_prefix.clone())
            .unwrap_or_else(|| global.archive_prefix.clone()),
        merge_strategy: project
            .and_then(|p| p.merge_strategy.clone())
            .unwrap_or_else(|| global.merge_strategy.clone()),
        worktree_dir,
        panes,
        setup_scripts: project.map(|p| p.scripts.setup.clone()).unwrap_or_default(),
        teardown_scripts: project
            .map(|p| p.scripts.teardown.clone())
            .unwrap_or_default(),
        issue_prompt: global.issue_prompt.clone(),
    }
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
        assert!(cmd.contains("sandbox_mode"));
        assert!(cmd.contains("workspace-write"));
        assert!(cmd.contains("approval_policy"));
    }

    #[test]
    fn resolve_agent_command_coder() {
        let cmd = resolve_agent_command("coder", None);
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
}
