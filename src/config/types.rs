use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum MergeStrategy {
    #[default]
    FfOnly,
    Merge,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SplitDirection {
    Right,
    Down,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PaneConfig {
    pub name: String,
    /// Agent to run in this pane (e.g., "claude", "codex", "every-code").
    /// If set, the command is auto-generated from the agent identifier.
    /// Mutually exclusive with `command`.
    pub agent: Option<String>,
    pub command: Option<String>,
    pub split_from: Option<String>,
    pub direction: Option<SplitDirection>,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// If true, this pane's command is deferred until after deferred setup
    /// scripts complete. The deferred setup scripts are prepended to this
    /// pane's command, chained with &&.
    #[serde(default)]
    pub deferred: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScriptConfig {
    pub name: String,
    pub command: String,
    pub working_dir: Option<String>,
    /// If true, this script runs in a terminal pane after the workspace is
    /// opened, rather than blocking before the workspace opens.
    #[serde(default)]
    pub deferred: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ScriptsConfig {
    #[serde(default)]
    pub setup: Vec<ScriptConfig>,
    #[serde(default)]
    pub teardown: Vec<ScriptConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PaneOverride {
    pub agent: Option<String>,
    pub command: Option<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// If true, defer this pane's command until after deferred setup scripts.
    #[serde(default)]
    pub deferred: bool,
}

/// A named port slot that foundry allocates per workspace.
pub type PortConfig = String;
