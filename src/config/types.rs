use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum MergeStrategy {
    #[default]
    FfOnly,
    Merge,
}

impl MergeStrategy {
    /// The config spelling, which is also what the history log records.
    pub fn as_str(&self) -> &'static str {
        match self {
            MergeStrategy::FfOnly => "ff-only",
            MergeStrategy::Merge => "merge",
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SplitDirection {
    Right,
    Down,
}

#[derive(Debug, Clone, Default, Deserialize)]
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `as_str` is the spelling a config is parsed from.
    #[test]
    fn merge_strategy_as_str_round_trips_through_serde() {
        for strategy in [MergeStrategy::FfOnly, MergeStrategy::Merge] {
            let parsed: MergeStrategy =
                serde::Deserialize::deserialize(serde::de::value::StrDeserializer::<
                    serde::de::value::Error,
                >::new(strategy.as_str()))
                .unwrap();
            assert_eq!(parsed, strategy);
        }
    }
}
