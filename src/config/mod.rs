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
    pub agent_command: String,
    pub archive_prefix: String,
    pub merge_strategy: MergeStrategy,
    pub worktree_dir: PathBuf,
    pub panes: Vec<PaneConfig>,
    pub setup_scripts: Vec<ScriptConfig>,
    pub teardown_scripts: Vec<ScriptConfig>,
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

    ResolvedConfig {
        branch_prefix: project
            .and_then(|p| p.branch_prefix.clone())
            .or_else(|| global.branch_prefix.clone()),
        agent_command: project
            .and_then(|p| p.agent_command.clone())
            .unwrap_or_else(|| global.agent_command.clone()),
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
