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
    });

    assert_eq!(state.list().len(), 2);
    state.prune_stale();
    assert_eq!(state.list().len(), 1);
    assert_eq!(state.list()[0].name, "valid");
}
