mod agents;
mod global;
mod project;
pub mod template;
pub mod types;
mod validation;

pub use agents::{
    AgentCapabilities, AgentInvocation, agent_capabilities, build_agent_command,
    check_agent_available, warn_agent_in_command,
};
pub use global::GlobalConfig;
pub use project::ProjectConfig;
pub use template::{TemplateVars, resolve_template, validate_template};
pub use types::{MergeStrategy, PaneConfig, PaneOverride, ScriptConfig, SplitDirection};
pub use validation::validate_panes;

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

impl ResolvedConfig {
    /// Override the primary agent (first agent pane) with a different agent.
    /// Used by `--agent` CLI flag for one-time agent overrides.
    pub fn override_primary_agent(&mut self, agent: &str) {
        // Update the first pane that has an agent configured
        if let Some(pane) = self.panes.iter_mut().find(|p| p.agent.is_some()) {
            pane.agent = Some(agent.to_string());
        }
        self.agent = agent.to_string();
    }
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
    if let Ok(raw) = toml::from_str::<toml::Value>(&contents) {
        validation::check_global_config_keys(&raw, &config_path.to_string_lossy());
    }

    let config: GlobalConfig = toml::from_str(&contents)
        .with_context(|| format!("failed to parse {}", config_path.display()))?;

    // Validate template variables in pane commands at parse time
    for pane in &config.panes {
        if let Some(ref cmd) = pane.command {
            validate_template(cmd).with_context(|| format!("in pane '{}' command", pane.name))?;
        }
        for key in pane.env.keys() {
            validation::validate_env_name(key, &format!("pane '{}' env", pane.name))?;
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
    if let Ok(raw) = toml::from_str::<toml::Value>(&contents) {
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

    // Env names reach a shell as bare identifiers in `export NAME=...`, so they
    // are validated rather than escaped. Both repo-settable sources are checked
    // here: pane env keys, and `ports` entries, which become an env name in
    // every pane.
    for (pane_name, pane) in &config.panes {
        for key in pane.env.keys() {
            validation::validate_env_name(key, &format!("pane '{pane_name}' env"))?;
        }
    }
    for port in &config.ports {
        validation::validate_env_name(port, "ports")?;
    }

    // The worktree path this produces is typed into a shell by the AppleScript
    // backends. That `cd` is quoted, so this is defence in depth.
    if let Some(ref dir) = config.worktree_dir {
        validation::validate_project_path(dir, "worktree_dir")?;
    }

    // This file is checked into the repository, so anything executable in it
    // was authored by the repo, not by the user running foundry. Gate it.
    crate::trust::ensure_trusted(repo_root, &contents, &config)?;

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
        unrestricted_permissions: resolve_unrestricted_permissions(
            global.unrestricted_permissions,
            project.and_then(|p| p.unrestricted_permissions),
        ),
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

/// Resolve `unrestricted_permissions` from the global and project settings.
///
/// Unlike every other scalar, the project value may only *narrow* this one.
/// `.foundry.toml` is checked into the repository, so letting it set the flag
/// to `true` would let repo content switch the agent into its most permissive
/// mode — `--dangerously-skip-permissions`, `--yolo`, `--trust-all-tools` —
/// behind the back of a user whose own config asked for restricted operation.
/// A project turning the flag *off* is a safe direction and still honoured.
fn resolve_unrestricted_permissions(global: bool, project: Option<bool>) -> bool {
    if !global && project == Some(true) {
        eprintln!(
            "Warning: .foundry.toml sets `unrestricted_permissions = true`, but your global \
             config does not. Ignoring — a project config cannot grant the agent more \
             permission than you configured. Set it in ~/.foundry/config.toml if you want it."
        );
        return false;
    }
    global && project.unwrap_or(true)
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

    /// A repo-supplied config must never be able to widen agent permissions.
    #[test]
    fn project_cannot_widen_unrestricted_permissions() {
        assert!(!resolve_unrestricted_permissions(false, Some(true)));
        assert!(!resolve_unrestricted_permissions(false, None));
        assert!(!resolve_unrestricted_permissions(false, Some(false)));
    }

    /// Narrowing is a safe direction, so a project may still opt out.
    #[test]
    fn project_can_narrow_unrestricted_permissions() {
        assert!(!resolve_unrestricted_permissions(true, Some(false)));
        assert!(resolve_unrestricted_permissions(true, Some(true)));
        assert!(resolve_unrestricted_permissions(true, None));
    }

    #[test]
    fn merge_ignores_project_attempt_to_widen_permissions() {
        let global = GlobalConfig {
            unrestricted_permissions: false,
            ..Default::default()
        };
        let project = ProjectConfig {
            unrestricted_permissions: Some(true),
            ..Default::default()
        };
        let resolved = merge_configs(&global, Some(&project));
        assert!(!resolved.unrestricted_permissions);
    }

    #[test]
    fn override_primary_agent_changes_first_agent_pane() {
        let global = GlobalConfig {
            agent: "claude".into(),
            ..Default::default()
        };
        let mut resolved = merge_configs(&global, None);
        assert_eq!(resolved.panes[0].agent.as_deref(), Some("claude"));

        resolved.override_primary_agent("crush");
        assert_eq!(resolved.panes[0].agent.as_deref(), Some("crush"));
        assert_eq!(resolved.agent, "crush");
    }

    #[test]
    fn override_primary_agent_only_changes_first_agent_pane() {
        let global = GlobalConfig {
            agent: "claude".into(),
            panes: vec![
                PaneConfig {
                    name: "agent".into(),
                    agent: Some("claude".into()),
                    command: None,
                    split_from: None,
                    direction: None,
                    optional: false,
                    env: Default::default(),
                    deferred: false,
                },
                PaneConfig {
                    name: "helper".into(),
                    agent: Some("codex".into()),
                    command: None,
                    split_from: Some("agent".into()),
                    direction: Some(SplitDirection::Right),
                    optional: false,
                    env: Default::default(),
                    deferred: false,
                },
            ],
            ..Default::default()
        };
        let mut resolved = merge_configs(&global, None);
        resolved.override_primary_agent("gemini");
        // First agent pane changed
        assert_eq!(resolved.panes[0].agent.as_deref(), Some("gemini"));
        // Second agent pane unchanged
        assert_eq!(resolved.panes[1].agent.as_deref(), Some("codex"));
    }
}
