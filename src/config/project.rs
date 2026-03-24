use super::types::{PaneOverride, ScriptsConfig};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct ProjectConfig {
    pub branch_prefix: Option<String>,
    pub agent: Option<String>,
    pub agent_command: Option<String>,
    pub archive_prefix: Option<String>,
    pub merge_strategy: Option<super::types::MergeStrategy>,
    pub worktree_dir: Option<String>,
    pub auto_fetch: Option<bool>,
    pub fetch_remote: Option<String>,
    /// Override the global shell setting for this project.
    pub shell: Option<String>,
    #[serde(default)]
    pub scripts: ScriptsConfig,
    #[serde(default)]
    pub panes: HashMap<String, PaneOverride>,
    #[serde(default)]
    /// Named port slots to allocate per workspace (e.g., ["VITE_PORT", "API_PORT"])
    pub ports: Vec<String>,
}
