//! Files foundry has already written to users' disks must keep loading.
//!
//! The round-trip tests elsewhere save and reload with the same build, so a
//! serde, toml, serde_json or chrono update that changed both directions at
//! once would still pass them while breaking every existing ~/.foundry. The
//! fixtures in tests/fixtures/persisted were written by foundry 0.6 (config
//! files by hand, in the documented format) and are never regenerated: if one
//! stops loading, that is a compatibility break, not a stale fixture. toml 0.9
//! did exactly this to `str::parse::<toml::Value>` — see `prescan_document`.

use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::{TimeZone, Utc};
use foundry::config::{self, GlobalConfig, MergeStrategy, ProjectConfig, SplitDirection};
use foundry::history::HistoryEvent;
use foundry::registry::Registry;
use foundry::state::WorkspaceState;
use foundry::trust::TrustStore;
use tempfile::TempDir;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/persisted")
        .join(name)
}

fn read_fixture(name: &str) -> String {
    std::fs::read_to_string(fixture(name)).unwrap()
}

#[test]
fn state_file_loads() {
    let state = WorkspaceState::load_from(&fixture("state.toml")).unwrap();
    let workspaces = state.list();
    assert_eq!(workspaces.len(), 3);

    let full = state.find("myapp", "login-form").unwrap();
    assert_eq!(full.branch, "xiphux/login-form");
    assert_eq!(
        full.worktree_path,
        "/home/dev/.foundry/worktrees/myapp/login-form"
    );
    assert_eq!(full.source_path, "/home/dev/src/myapp");
    assert_eq!(
        full.created_at,
        Utc.with_ymd_and_hms(2026, 9, 14, 21, 30, 5).unwrap()
    );
    assert_eq!(full.terminal_tab_id, "tab-3");
    assert_eq!(full.allocated_ports.get("VITE_PORT"), Some(&10042));
    assert_eq!(full.allocated_ports.get("API_PORT"), Some(&10043));
    assert_eq!(full.pr_number, Some(128));
    assert_eq!(
        full.pr_url.as_deref(),
        Some("https://github.com/xiphux/myapp/pull/128")
    );

    let no_pr = state.find("tool", "fix-crash").unwrap();
    assert_eq!(no_pr.terminal_tab_id, "");
    assert!(no_pr.allocated_ports.is_empty());
    assert_eq!(no_pr.pr_number, None);

    let old = state.find("tool", "old-entry").unwrap();
    assert_eq!(old.terminal_tab_id, "");
    assert!(old.allocated_ports.is_empty());
    assert_eq!(old.pr_url, None);
}

#[test]
fn state_file_survives_a_save() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("state.toml");
    WorkspaceState::load_from(&fixture("state.toml"))
        .unwrap()
        .save_to(&path)
        .unwrap();
    let reloaded = WorkspaceState::load_from(&path).unwrap();
    assert_eq!(reloaded.list().len(), 3);
    assert_eq!(
        reloaded.find("myapp", "login-form").unwrap().pr_number,
        Some(128)
    );
}

#[test]
fn registry_file_loads() {
    let registry = Registry::load_from(&fixture("projects.toml")).unwrap();
    assert_eq!(
        registry.list(),
        vec![
            ("myapp".to_string(), PathBuf::from("/home/dev/src/myapp")),
            ("tool".to_string(), PathBuf::from("/home/dev/src/tool")),
        ]
    );
}

#[test]
fn trust_file_loads() {
    // Keys are canonical project roots, which differ per machine, so the
    // fixture's placeholder is swapped for a real directory.
    let root = TempDir::new().unwrap();
    let key = foundry::fs_util::canonicalize_existing_prefix(root.path());
    let contents =
        read_fixture("trust.toml").replace("@ROOT@", &key.to_string_lossy().replace('\\', "\\\\"));
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("trust.toml");
    std::fs::write(&path, contents).unwrap();

    let store = TrustStore::load_from(&path).unwrap();
    let hash = "3f5e1c1b7a3d9e8f2b6c4a0d5e7f9a1b3c5d7e9f1a3b5c7d9e1f3a5b7c9d1e3f";
    assert!(store.is_trusted(root.path(), hash));
    assert!(!store.is_trusted(root.path(), "0000"));
}

#[test]
fn history_log_parses_every_event_kind() {
    let events: Vec<HistoryEvent> = read_fixture("history.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap_or_else(|e| panic!("{line}: {e}")))
        .collect();
    let kinds: Vec<&str> = events.iter().map(|e| e.event.as_str()).collect();
    assert_eq!(
        kinds,
        [
            "started",
            "pr_created",
            "pr_merged",
            "finished",
            "started",
            "discarded",
            "restored"
        ]
    );
    let t = Utc.with_ymd_and_hms(2026, 9, 14, 21, 30, 5).unwrap();
    assert!(events.iter().all(|e| e.timestamp == t));
    assert_eq!(events[0].from_issue.as_deref(), Some("42"));
    assert_eq!(
        events[1].pr_url.as_deref(),
        Some("https://github.com/xiphux/myapp/pull/128")
    );
    assert_eq!(events[3].commits, Some(3));
    assert_eq!(events[3].merge_strategy.as_deref(), Some("ff-only"));
    assert_eq!(events[4].from_issue, None);
    assert_eq!(
        events[5].archived_as.as_deref(),
        Some("archive/fix-crash-20260914")
    );
    assert_eq!(
        events[6].from_branch.as_deref(),
        Some("archive/fix-crash-20260914")
    );
}

/// Through the binary, so the log's real reader (which parses backward from
/// the end of the file) and the `history` display are what get exercised.
#[test]
fn history_command_reads_the_log() {
    let home = TempDir::new().unwrap();
    std::fs::copy(fixture("history.jsonl"), home.path().join("history.jsonl")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_foundry"))
        .args(["history", "--limit", "3"])
        .env("FOUNDRY_HOME", home.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 3, "{stdout}");
    assert!(stdout.contains("tool/fix-crash"), "{stdout}");
    assert!(
        stdout.contains("from archive/fix-crash-20260914"),
        "{stdout}"
    );
    assert!(
        stdout.contains("archived as archive/fix-crash-20260914"),
        "{stdout}"
    );
    assert!(stdout.contains("2026-09-14 21:30"), "{stdout}");
}

#[test]
fn global_config_loads_every_key() {
    let contents = read_fixture("config.toml");
    let global: GlobalConfig = toml::from_str(&contents).unwrap();
    assert_eq!(global.branch_prefix.as_deref(), Some("xiphux"));
    assert_eq!(global.merge_strategy, MergeStrategy::Merge);
    assert_eq!(global.port_range_start, Some(20000));
    assert!(global.auto_fetch);
    assert_eq!(global.fetch_remote.as_deref(), Some("upstream"));
    assert_eq!(
        global.issue_prompt.as_deref(),
        Some("Fix #{issue_number}: {title}\n\n{body}")
    );
    assert_eq!(global.editor.as_deref(), Some("zed"));

    let names: Vec<&str> = global.panes.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["agent", "git", "shell", "server"]);
    assert_eq!(global.panes[0].agent.as_deref(), Some("claude"));
    assert_eq!(
        global.panes[0].env.get("SOME_VAR").map(String::as_str),
        Some("value")
    );
    assert_eq!(global.panes[1].command.as_deref(), Some("lazygit"));
    assert_eq!(global.panes[1].direction, Some(SplitDirection::Right));
    assert_eq!(global.panes[2].direction, Some(SplitDirection::Down));
    assert!(global.panes[2].deferred);
    assert!(global.panes[3].optional);

    for pane in &global.panes {
        if let Some(cmd) = &pane.command {
            config::validate_template(cmd).unwrap();
        }
    }
    config::validate_panes(&global.panes).unwrap();
}

#[test]
fn project_config_loads_every_key_and_merges() {
    let global: GlobalConfig = toml::from_str(&read_fixture("config.toml")).unwrap();
    let project: ProjectConfig = toml::from_str(&read_fixture("foundry.toml")).unwrap();

    assert_eq!(project.scripts.setup.len(), 2);
    assert_eq!(
        project.scripts.setup[1].working_dir.as_deref(),
        Some("{worktree}")
    );
    assert!(project.scripts.setup[1].deferred);
    assert_eq!(project.scripts.teardown[0].name, "Stop containers");
    assert_eq!(project.ports, ["VITE_PORT", "API_PORT"]);

    let resolved = config::merge_configs(&global, Some(&project));
    assert_eq!(resolved.branch_prefix.as_deref(), Some("team"));
    assert_eq!(resolved.agent, "codex");
    assert_eq!(resolved.archive_prefix, "old");
    assert_eq!(resolved.merge_strategy, MergeStrategy::FfOnly);
    assert_eq!(
        resolved.worktree_dir,
        PathBuf::from("/home/dev/project-worktrees")
    );
    assert!(!resolved.auto_fetch);
    assert_eq!(resolved.pr_remote.as_deref(), Some("fork"));
    assert_eq!(resolved.shell.as_deref(), Some("zsh"));
    assert_eq!(resolved.port_range_start, 20000);
    assert_eq!(
        resolved.context.as_deref(),
        Some("The dev server runs at http://localhost:{VITE_PORT}")
    );

    // The optional server pane is opted in by the project's override.
    let server = resolved.panes.iter().find(|p| p.name == "server").unwrap();
    assert_eq!(server.command.as_deref(), Some("npm run dev"));
    assert_eq!(
        server.env.get("NODE_ENV").map(String::as_str),
        Some("development")
    );
    assert!(
        resolved
            .panes
            .iter()
            .find(|p| p.name == "git")
            .unwrap()
            .deferred
    );

    for script in resolved
        .setup_scripts
        .iter()
        .chain(&resolved.teardown_scripts)
    {
        config::validate_template(&script.command).unwrap();
    }
    config::validate_panes(&resolved.panes).unwrap();
}
