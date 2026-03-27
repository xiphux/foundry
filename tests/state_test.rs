use chrono::Utc;
use tempfile::TempDir;

#[test]
fn test_state_add_and_list() {
    let dir = TempDir::new().unwrap();
    let state_path = dir.path().join("state.toml");
    let mut state = foundry::state::WorkspaceState::load_from(&state_path).unwrap();
    state.add(foundry::state::Workspace {
        project: "myapp".into(),
        name: "my-feature".into(),
        branch: "xiphux/my-feature".into(),
        worktree_path: "/tmp/worktrees/myapp/my-feature".into(),
        source_path: "/code/myapp".into(),
        created_at: chrono::Utc::now(),
        terminal_tab_id: String::new(),
        allocated_ports: Default::default(),
        pr_number: None,
        pr_url: None,
    });
    state.save_to(&state_path).unwrap();
    let reloaded = foundry::state::WorkspaceState::load_from(&state_path).unwrap();
    assert_eq!(reloaded.list().len(), 1);
    assert_eq!(reloaded.list()[0].name, "my-feature");
}

#[test]
fn test_state_remove() {
    let dir = TempDir::new().unwrap();
    let state_path = dir.path().join("state.toml");
    let mut state = foundry::state::WorkspaceState::load_from(&state_path).unwrap();
    state.add(foundry::state::Workspace {
        project: "myapp".into(),
        name: "feat-a".into(),
        branch: "feat-a".into(),
        worktree_path: "/tmp/worktrees/myapp/feat-a".into(),
        source_path: "/code/myapp".into(),
        created_at: chrono::Utc::now(),
        terminal_tab_id: String::new(),
        allocated_ports: Default::default(),
        pr_number: None,
        pr_url: None,
    });
    state.remove("myapp", "feat-a");
    state.save_to(&state_path).unwrap();
    let reloaded = foundry::state::WorkspaceState::load_from(&state_path).unwrap();
    assert!(reloaded.list().is_empty());
}

#[test]
fn test_state_find_by_project() {
    let dir = TempDir::new().unwrap();
    let state_path = dir.path().join("state.toml");
    let mut state = foundry::state::WorkspaceState::load_from(&state_path).unwrap();
    state.add(foundry::state::Workspace {
        project: "myapp".into(),
        name: "feat-a".into(),
        branch: "feat-a".into(),
        worktree_path: "/tmp/worktrees/myapp/feat-a".into(),
        source_path: "/code/myapp".into(),
        created_at: chrono::Utc::now(),
        terminal_tab_id: String::new(),
        allocated_ports: Default::default(),
        pr_number: None,
        pr_url: None,
    });
    state.add(foundry::state::Workspace {
        project: "other".into(),
        name: "feat-b".into(),
        branch: "feat-b".into(),
        worktree_path: "/tmp/worktrees/other/feat-b".into(),
        source_path: "/code/other".into(),
        created_at: chrono::Utc::now(),
        terminal_tab_id: String::new(),
        allocated_ports: Default::default(),
        pr_number: None,
        pr_url: None,
    });
    let myapp_workspaces = state.find_by_project("myapp");
    assert_eq!(myapp_workspaces.len(), 1);
    assert_eq!(myapp_workspaces[0].name, "feat-a");
}

#[test]
fn test_state_find_by_worktree_path() {
    let dir = TempDir::new().unwrap();
    let state_path = dir.path().join("state.toml");
    let mut state = foundry::state::WorkspaceState::load_from(&state_path).unwrap();
    state.add(foundry::state::Workspace {
        project: "myapp".into(),
        name: "feat-a".into(),
        branch: "feat-a".into(),
        worktree_path: "/tmp/worktrees/myapp/feat-a".into(),
        source_path: "/code/myapp".into(),
        created_at: chrono::Utc::now(),
        terminal_tab_id: String::new(),
        allocated_ports: Default::default(),
        pr_number: None,
        pr_url: None,
    });
    let found = state.find_by_worktree_path("/tmp/worktrees/myapp/feat-a");
    assert!(found.is_some());
    assert_eq!(found.unwrap().name, "feat-a");
    let found = state.find_by_worktree_path("/tmp/worktrees/myapp/feat-a/src");
    assert!(found.is_some());
}

#[test]
fn test_set_terminal_tab_id() {
    let dir = TempDir::new().unwrap();
    let state_path = dir.path().join("state.toml");
    let mut state = foundry::state::WorkspaceState::load_from(&state_path).unwrap();
    state.add(foundry::state::Workspace {
        project: "myapp".into(),
        name: "feat-a".into(),
        branch: "feat-a".into(),
        worktree_path: "/tmp/worktrees/myapp/feat-a".into(),
        source_path: "/code/myapp".into(),
        created_at: chrono::Utc::now(),
        terminal_tab_id: String::new(),
        allocated_ports: Default::default(),
        pr_number: None,
        pr_url: None,
    });
    state.set_terminal_tab_id("myapp", "feat-a", "tab-123".to_string());
    state.save_to(&state_path).unwrap();

    let reloaded = foundry::state::WorkspaceState::load_from(&state_path).unwrap();
    assert_eq!(reloaded.list()[0].terminal_tab_id, "tab-123");
}

#[test]
fn test_prune_stale() {
    let dir = TempDir::new().unwrap();
    let state_path = dir.path().join("state.toml");

    // Create a real directory for the "valid" workspace
    let valid_dir = dir.path().join("valid-worktree");
    std::fs::create_dir(&valid_dir).unwrap();

    let mut state = foundry::state::WorkspaceState::load_from(&state_path).unwrap();
    state.add(foundry::state::Workspace {
        project: "myapp".into(),
        name: "valid".into(),
        branch: "valid".into(),
        worktree_path: valid_dir.to_string_lossy().into(),
        source_path: "/code/myapp".into(),
        created_at: chrono::Utc::now(),
        terminal_tab_id: String::new(),
        allocated_ports: Default::default(),
        pr_number: None,
        pr_url: None,
    });
    state.add(foundry::state::Workspace {
        project: "myapp".into(),
        name: "stale".into(),
        branch: "stale".into(),
        worktree_path: "/nonexistent/path/that/does/not/exist".into(),
        source_path: "/code/myapp".into(),
        created_at: chrono::Utc::now(),
        terminal_tab_id: String::new(),
        allocated_ports: Default::default(),
        pr_number: None,
        pr_url: None,
    });

    assert_eq!(state.list().len(), 2);
    state.prune_stale();
    assert_eq!(state.list().len(), 1);
    assert_eq!(state.list()[0].name, "valid");
}

#[test]
fn test_set_pr_info() {
    let dir = TempDir::new().unwrap();
    let state_path = dir.path().join("state.toml");
    let mut state = foundry::state::WorkspaceState::load_from(&state_path).unwrap();

    state.add(foundry::state::Workspace {
        project: "myapp".into(),
        name: "feat".into(),
        branch: "feat".into(),
        worktree_path: "/tmp/test-worktree".into(),
        source_path: "/tmp/test-source".into(),
        created_at: Utc::now(),
        terminal_tab_id: String::new(),
        allocated_ports: std::collections::HashMap::new(),
        pr_number: None,
        pr_url: None,
    });

    state.set_pr_info("myapp", "feat", 42, "https://github.com/user/repo/pull/42");

    let ws = state.list().iter().find(|w| w.name == "feat").unwrap();
    assert_eq!(ws.pr_number, Some(42));
    assert_eq!(
        ws.pr_url.as_deref(),
        Some("https://github.com/user/repo/pull/42")
    );
}

#[test]
fn test_clear_pr_info() {
    let dir = TempDir::new().unwrap();
    let state_path = dir.path().join("state.toml");
    let mut state = foundry::state::WorkspaceState::load_from(&state_path).unwrap();

    state.add(foundry::state::Workspace {
        project: "myapp".into(),
        name: "feat".into(),
        branch: "feat".into(),
        worktree_path: "/tmp/test-worktree".into(),
        source_path: "/tmp/test-source".into(),
        created_at: Utc::now(),
        terminal_tab_id: String::new(),
        allocated_ports: std::collections::HashMap::new(),
        pr_number: Some(42),
        pr_url: Some("https://github.com/user/repo/pull/42".into()),
    });

    state.clear_pr_info("myapp", "feat");

    let ws = state.list().iter().find(|w| w.name == "feat").unwrap();
    assert_eq!(ws.pr_number, None);
    assert_eq!(ws.pr_url, None);
}

#[test]
fn test_set_pr_info_wrong_workspace_is_noop() {
    let dir = TempDir::new().unwrap();
    let state_path = dir.path().join("state.toml");
    let mut state = foundry::state::WorkspaceState::load_from(&state_path).unwrap();

    state.add(foundry::state::Workspace {
        project: "myapp".into(),
        name: "feat".into(),
        branch: "feat".into(),
        worktree_path: "/tmp/test-worktree".into(),
        source_path: "/tmp/test-source".into(),
        created_at: Utc::now(),
        terminal_tab_id: String::new(),
        allocated_ports: std::collections::HashMap::new(),
        pr_number: None,
        pr_url: None,
    });

    // Set PR info for a non-existent workspace — should be a no-op
    state.set_pr_info("myapp", "nonexistent", 99, "https://example.com");

    let ws = state.list().iter().find(|w| w.name == "feat").unwrap();
    assert_eq!(ws.pr_number, None);
}

#[test]
fn test_pr_info_survives_save_load() {
    let dir = TempDir::new().unwrap();
    let state_path = dir.path().join("state.toml");
    let mut state = foundry::state::WorkspaceState::load_from(&state_path).unwrap();

    state.add(foundry::state::Workspace {
        project: "myapp".into(),
        name: "feat".into(),
        branch: "feat".into(),
        worktree_path: "/tmp/test-worktree".into(),
        source_path: "/tmp/test-source".into(),
        created_at: Utc::now(),
        terminal_tab_id: String::new(),
        allocated_ports: std::collections::HashMap::new(),
        pr_number: Some(42),
        pr_url: Some("https://github.com/user/repo/pull/42".into()),
    });

    state.save_to(&state_path).unwrap();

    let reloaded = foundry::state::WorkspaceState::load_from(&state_path).unwrap();
    let ws = reloaded.list().iter().find(|w| w.name == "feat").unwrap();
    assert_eq!(ws.pr_number, Some(42));
    assert_eq!(
        ws.pr_url.as_deref(),
        Some("https://github.com/user/repo/pull/42")
    );
}

#[test]
fn test_pr_info_none_not_serialized() {
    let dir = TempDir::new().unwrap();
    let state_path = dir.path().join("state.toml");
    let mut state = foundry::state::WorkspaceState::load_from(&state_path).unwrap();

    state.add(foundry::state::Workspace {
        project: "myapp".into(),
        name: "feat".into(),
        branch: "feat".into(),
        worktree_path: "/tmp/test-worktree".into(),
        source_path: "/tmp/test-source".into(),
        created_at: Utc::now(),
        terminal_tab_id: String::new(),
        allocated_ports: std::collections::HashMap::new(),
        pr_number: None,
        pr_url: None,
    });

    state.save_to(&state_path).unwrap();

    let contents = std::fs::read_to_string(&state_path).unwrap();
    assert!(
        !contents.contains("pr_number"),
        "None pr_number should not appear in TOML"
    );
    assert!(
        !contents.contains("pr_url"),
        "None pr_url should not appear in TOML"
    );
}
