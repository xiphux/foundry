use super::types::{MergeStrategy, PaneConfig, SplitDirection};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct GlobalConfig {
    pub branch_prefix: Option<String>,
    pub agent_command: String,
    pub archive_prefix: String,
    pub merge_strategy: MergeStrategy,
    pub worktree_dir: String,
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
        },
        PaneConfig {
            name: "shell".into(),
            command: None,
            split_from: Some("agent".into()),
            direction: Some(SplitDirection::Right),
            optional: false,
            env: Default::default(),
        },
    ]
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            branch_prefix: None,
            agent_command: "claude".into(),
            archive_prefix: "archive".into(),
            merge_strategy: MergeStrategy::default(),
            worktree_dir: "~/.foundry/worktrees".into(),
            panes: default_panes(),
        }
    }
}
