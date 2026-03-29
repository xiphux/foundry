mod agents;
mod global;
mod project;
pub mod template;
pub mod types;
mod validation;

pub use agents::{
    AgentCapabilities, agent_capabilities, build_agent_command, check_agent_available,
    resolve_agent_command, warn_agent_in_command,
};
pub use global::GlobalConfig;
pub use project::ProjectConfig;
pub use template::{TemplateVars, resolve_template, validate_template};
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
    /// If true, agents use their most permissive mode, bypassing all permission prompts.
    pub unrestricted_permissions: bool,
    /// Editor command for `foundry edit` (e.g., "code", "cursor", "zed").
    pub editor: Option<String>,
    /// Shell executable to use in terminal panes (e.g., "bash", "powershell").
    pub shell: Option<String>,
    /// Starting port for dynamic port allocation (default: 10000)
    pub port_range_start: u16,
    /// Named port slots to allocate per workspace (env var names)
    pub ports: Vec<String>,
    /// User-configured context message appended to the agent system prompt.
    pub context: Option<String>,
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

    // Check for unknown keys before deserializing
    if let Ok(raw) = contents.parse::<toml::Value>() {
        validation::check_global_config_keys(&raw, &config_path.to_string_lossy());
    }

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

    // Check for unknown keys before deserializing
    if let Ok(raw) = contents.parse::<toml::Value>() {
        validation::check_project_config_keys(&raw, &config_path.to_string_lossy());
    }

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
        unrestricted_permissions: project
            .and_then(|p| p.unrestricted_permissions)
            .unwrap_or(global.unrestricted_permissions),
        editor: global.editor.clone(),
        shell: project
            .and_then(|p| p.shell.clone())
            .or_else(|| global.shell.clone()),
        port_range_start: global.port_range_start.unwrap_or(10000),
        ports: project.map(|p| p.ports.clone()).unwrap_or_default(),
        context: project.and_then(|p| p.context.clone()),
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
}
