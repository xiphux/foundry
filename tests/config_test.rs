#[test]
fn test_global_config_deserialization() {
    let toml_str = r#"
branch_prefix = "xiphux"
agent = "claude"
archive_prefix = "archive"
merge_strategy = "ff-only"
worktree_dir = "~/.foundry/worktrees"

[[panes]]
name = "agent"
agent = "claude"

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
    assert_eq!(config.agent, "claude");
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
    assert_eq!(config.agent, "claude");
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
                agent: Some("claude".into()),
                command: None,
                split_from: None,
                direction: None,
                optional: false,
                env: Default::default(),
                deferred: false,
            },
            foundry::config::PaneConfig {
                name: "server".into(),
                agent: None,
                command: Some("npm run dev".into()),
                split_from: Some("agent".into()),
                direction: Some(foundry::config::SplitDirection::Right),
                optional: true,
                env: Default::default(),
                deferred: false,
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
                agent: Some("claude".into()),
                command: None,
                split_from: None,
                direction: None,
                optional: false,
                env: Default::default(),
                deferred: false,
            },
            foundry::config::PaneConfig {
                name: "server".into(),
                agent: None,
                command: Some("npm run dev".into()),
                split_from: Some("agent".into()),
                direction: Some(foundry::config::SplitDirection::Right),
                optional: true,
                env: Default::default(),
                deferred: false,
            },
        ],
        ..Default::default()
    };
    let project = foundry::config::ProjectConfig {
        panes: std::collections::HashMap::from([(
            "server".into(),
            foundry::config::PaneOverride {
                agent: None,
                command: Some("npm run serve".into()),
                env: Default::default(),
                deferred: false,
            },
        )]),
        ..Default::default()
    };
    let resolved = foundry::config::merge_configs(&global, Some(&project));
    assert_eq!(resolved.panes.len(), 2);
    assert_eq!(resolved.panes[1].command.as_deref(), Some("npm run serve"));
}

#[test]
fn test_build_agent_command_claude_with_prompt() {
    let cmd = foundry::config::build_agent_command(
        "claude",
        None,
        Some("fix the auth bug"),
        false,
        false,
        None,
    );
    assert!(cmd.contains("claude"));
    assert!(cmd.contains("'fix the auth bug'"));
}

#[test]
fn test_build_agent_command_claude_without_prompt() {
    let cmd = foundry::config::build_agent_command("claude", None, None, false, false, None);
    assert!(cmd.starts_with("claude"));
    assert!(!cmd.contains("'"));
}

#[test]
fn test_build_agent_command_prompt_with_quotes() {
    let cmd = foundry::config::build_agent_command(
        "claude",
        None,
        Some("fix the user's auth bug"),
        false,
        false,
        None,
    );
    assert!(cmd.contains("fix the user"));
    assert!(cmd.contains("auth bug"));
}

#[test]
fn test_build_agent_command_claude_continue() {
    let cmd = foundry::config::build_agent_command("claude", None, None, true, false, None);
    assert!(cmd.contains("--continue"));
}

#[test]
fn test_build_agent_command_codex_resume() {
    let cmd = foundry::config::build_agent_command("codex", None, None, true, false, None);
    assert!(cmd.contains("--resume"));
}

#[test]
fn test_build_agent_command_continue_with_prompt() {
    let cmd = foundry::config::build_agent_command(
        "claude",
        None,
        Some("do something"),
        true,
        false,
        None,
    );
    assert!(cmd.contains("--continue"));
    assert!(cmd.contains("do something"));
}

#[test]
fn test_default_layout_gets_global_agent() {
    let global = foundry::config::GlobalConfig {
        agent: "codex".into(),
        ..Default::default()
    };
    let resolved = foundry::config::merge_configs(&global, None);
    // The default layout's first pane should get the global agent
    assert_eq!(resolved.panes[0].agent.as_deref(), Some("codex"));
}

#[test]
fn test_custom_panes_with_agent_override_global() {
    let global = foundry::config::GlobalConfig {
        agent: "claude".into(),
        panes: vec![
            foundry::config::PaneConfig {
                name: "dev".into(),
                agent: Some("codex".into()),
                command: None,
                split_from: None,
                direction: None,
                optional: false,
                env: Default::default(),
                deferred: false,
            },
            foundry::config::PaneConfig {
                name: "shell".into(),
                agent: None,
                command: None,
                split_from: Some("dev".into()),
                direction: Some(foundry::config::SplitDirection::Right),
                optional: false,
                env: Default::default(),
                deferred: false,
            },
        ],
        ..Default::default()
    };
    let resolved = foundry::config::merge_configs(&global, None);
    // Pane has explicit agent, global agent should NOT be applied to any pane
    assert_eq!(resolved.panes[0].agent.as_deref(), Some("codex"));
    assert_eq!(resolved.panes[1].agent, None);
}
