use super::types::{MergeStrategy, PaneConfig, SplitDirection};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct GlobalConfig {
    pub branch_prefix: Option<String>,
    /// Known agent identifier: "claude", "codex", or "custom"
    pub agent: String,
    /// Custom agent command (only used when agent = "custom")
    pub agent_command: Option<String>,
    pub archive_prefix: String,
    pub merge_strategy: MergeStrategy,
    pub worktree_dir: String,
    /// Custom prompt template for GitHub issues.
    /// Supports variables: {issue_number}, {title}, {body}
    pub issue_prompt: Option<String>,
    #[serde(default = "default_panes")]
    pub panes: Vec<PaneConfig>,
}

fn default_panes() -> Vec<PaneConfig> {
    vec![
        PaneConfig {
            name: "agent".into(),
            command: Some("{agent_command}".into()),
            split_from: None,
            direction: None,
            optional: false,
            env: Default::default(),
            deferred: false,
        },
        PaneConfig {
            name: "shell".into(),
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
            issue_prompt: None,
            panes: default_panes(),
        }
    }
}
