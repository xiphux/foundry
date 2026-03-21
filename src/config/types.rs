use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum MergeStrategy {
    FfOnly,
    Merge,
}

impl Default for MergeStrategy {
    fn default() -> Self {
        Self::FfOnly
    }
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
    pub command: Option<String>,
    pub split_from: Option<String>,
    pub direction: Option<SplitDirection>,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScriptConfig {
    pub name: String,
    pub command: String,
    pub working_dir: Option<String>,
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
    pub command: Option<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
}
