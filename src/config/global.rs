use super::types::{MergeStrategy, PaneConfig, SplitDirection};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct GlobalConfig {
    pub branch_prefix: Option<String>,
    /// Known agent identifier: "claude", "codex", "every-code", or "custom"
    pub agent: String,
    /// Custom agent command (only used when agent = "custom")
    pub agent_command: Option<String>,
    pub archive_prefix: String,
    pub merge_strategy: MergeStrategy,
    pub worktree_dir: String,
    /// Starting port for dynamic port allocation (default: 10000)
    pub port_range_start: Option<u16>,
    /// Automatically fetch and fast-forward main from remote before branching
    #[serde(default)]
    pub auto_fetch: bool,
    /// Remote name to fetch from (default: "origin")
    pub fetch_remote: Option<String>,
    /// Custom prompt template for GitHub issues.
    /// Supports variables: {issue_number}, {title}, {body}
    pub issue_prompt: Option<String>,
    /// Shell executable to use in terminal panes (e.g., "bash", "powershell").
    /// On Windows Terminal, defaults to "powershell" if unset.
    /// Other terminal backends use their default shell and ignore this setting.
    pub shell: Option<String>,
    #[serde(default = "default_panes")]
    pub panes: Vec<PaneConfig>,
}

fn default_panes() -> Vec<PaneConfig> {
    vec![
        PaneConfig {
            name: "agent".into(),
            agent: None, // filled from global agent field at merge time
            command: None,
            split_from: None,
            direction: None,
            optional: false,
            env: Default::default(),
            deferred: false,
        },
        PaneConfig {
            name: "shell".into(),
            agent: None,
            command: None,
            split_from: Some("agent".into()),
            direction: Some(SplitDirection::Right),
            optional: false,
            env: Default::default(),
            deferred: false,
        },
    ]
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            branch_prefix: None,
            agent: "claude".into(),
            agent_command: None,
            archive_prefix: "archive".into(),
            merge_strategy: MergeStrategy::default(),
            worktree_dir: "~/.foundry/worktrees".into(),
            port_range_start: None,
            auto_fetch: false,
            fetch_remote: None,
            issue_prompt: None,
            shell: None,
            panes: default_panes(),
        }
    }
}
