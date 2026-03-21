#[test]
fn test_global_config_deserialization() {
    let toml_str = r#"
branch_prefix = "xiphux"
agent_command = "claude"
archive_prefix = "archive"
merge_strategy = "ff-only"
worktree_dir = "~/.foundry/worktrees"

[[panes]]
name = "agent"
command = "{agent_command}"

[[panes]]
name = "git"
command = "lazygit"
split_from = "agent"
direction = "right"

[[panes]]
name = "shell"
split_from = "git"
direction = "down"

[[panes]]
name = "server"
split_from = "shell"
direction = "right"
optional = true
"#;
    let config: foundry::config::GlobalConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.branch_prefix.as_deref(), Some("xiphux"));
    assert_eq!(config.agent_command, "claude");
    assert_eq!(
        config.merge_strategy,
        foundry::config::MergeStrategy::FfOnly
    );
    assert_eq!(config.panes.len(), 4);
    assert!(config.panes[3].optional);
}

#[test]
fn test_global_config_defaults() {
    let toml_str = "";
    let config: foundry::config::GlobalConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.branch_prefix, None);
    assert_eq!(config.agent_command, "claude");
    assert_eq!(config.archive_prefix, "archive");
    assert_eq!(
        config.merge_strategy,
        foundry::config::MergeStrategy::FfOnly
    );
}

#[test]
fn test_project_config_deserialization() {
    let toml_str = r#"
[[scripts.setup]]
name = "Copy env"
command = "cp {source}/.env {worktree}/.env"

[[scripts.teardown]]
name = "Docker down"
command = "docker compose down"
working_dir = "{worktree}"

[panes.server]
command = "npm run serve"
"#;
    let config: foundry::config::ProjectConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.scripts.setup.len(), 1);
    assert_eq!(config.scripts.teardown.len(), 1);
    assert!(config.panes.contains_key("server"));
}

#[test]
fn test_merge_strategy_deserialization() {
    let toml_str = r#"merge_strategy = "merge""#;
    let config: foundry::config::GlobalConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.merge_strategy, foundry::config::MergeStrategy::Merge);
}

#[test]
fn test_config_merge_optional_pane_skipped() {
    let global = foundry::config::GlobalConfig {
        panes: vec![
            foundry::config::PaneConfig {
                name: "agent".into(),
                command: Some("claude".into()),
                split_from: None,
                direction: None,
                optional: false,
                env: Default::default(),
            },
            foundry::config::PaneConfig {
                name: "server".into(),
                command: Some("npm run dev".into()),
                split_from: Some("agent".into()),
                direction: Some(foundry::config::SplitDirection::Right),
                optional: true,
                env: Default::default(),
            },
        ],
        ..Default::default()
    };
    let resolved = foundry::config::merge_configs(&global, None);
    assert_eq!(resolved.panes.len(), 1);
    assert_eq!(resolved.panes[0].name, "agent");
}

#[test]
fn test_config_merge_optional_pane_opted_in() {
    let global = foundry::config::GlobalConfig {
        panes: vec![
            foundry::config::PaneConfig {
                name: "agent".into(),
                command: Some("claude".into()),
                split_from: None,
                direction: None,
                optional: false,
                env: Default::default(),
            },
            foundry::config::PaneConfig {
                name: "server".into(),
                command: Some("npm run dev".into()),
                split_from: Some("agent".into()),
                direction: Some(foundry::config::SplitDirection::Right),
                optional: true,
                env: Default::default(),
            },
        ],
        ..Default::default()
    };
    let project = foundry::config::ProjectConfig {
        panes: std::collections::HashMap::from([(
            "server".into(),
            foundry::config::PaneOverride {
                command: Some("npm run serve".into()),
                env: Default::default(),
            },
        )]),
        ..Default::default()
    };
    let resolved = foundry::config::merge_configs(&global, Some(&project));
    assert_eq!(resolved.panes.len(), 2);
    assert_eq!(resolved.panes[1].command.as_deref(), Some("npm run serve"));
}
