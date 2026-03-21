use serde::Deserialize;
use super::types::{MergeStrategy, PaneConfig};

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct GlobalConfig {
    pub branch_prefix: Option<String>,
    pub agent_command: String,
    pub archive_prefix: String,
    pub merge_strategy: MergeStrategy,
    pub worktree_dir: String,
    #[serde(default)]
    pub panes: Vec<PaneConfig>,
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            branch_prefix: None,
            agent_command: "claude".into(),
            archive_prefix: "archive".into(),
            merge_strategy: MergeStrategy::default(),
            worktree_dir: "~/.foundry/worktrees".into(),
            panes: Vec::new(),
        }
    }
}
