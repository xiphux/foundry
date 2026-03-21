use tempfile::TempDir;

#[test]
fn test_state_add_and_list() {
    let dir = TempDir::new().unwrap();
    let state_path = dir.path().join("state.toml");
    let mut state = foundry::state::WorkspaceState::load_from(&state_path).unwrap();
    state.add(foundry::state::Workspace {
        project: "myapp".into(), name: "my-feature".into(), branch: "xiphux/my-feature".into(),
        worktree_path: "/tmp/worktrees/myapp/my-feature".into(), source_path: "/code/myapp".into(),
        created_at: chrono::Utc::now(), terminal_tab_id: String::new(),
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
        project: "myapp".into(), name: "feat-a".into(), branch: "feat-a".into(),
        worktree_path: "/tmp/worktrees/myapp/feat-a".into(), source_path: "/code/myapp".into(),
        created_at: chrono::Utc::now(), terminal_tab_id: String::new(),
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
        project: "myapp".into(), name: "feat-a".into(), branch: "feat-a".into(),
        worktree_path: "/tmp/worktrees/myapp/feat-a".into(), source_path: "/code/myapp".into(),
        created_at: chrono::Utc::now(), terminal_tab_id: String::new(),
    });
    state.add(foundry::state::Workspace {
        project: "other".into(), name: "feat-b".into(), branch: "feat-b".into(),
        worktree_path: "/tmp/worktrees/other/feat-b".into(), source_path: "/code/other".into(),
        created_at: chrono::Utc::now(), terminal_tab_id: String::new(),
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
        project: "myapp".into(), name: "feat-a".into(), branch: "feat-a".into(),
        worktree_path: "/tmp/worktrees/myapp/feat-a".into(), source_path: "/code/myapp".into(),
        created_at: chrono::Utc::now(), terminal_tab_id: String::new(),
    });
    let found = state.find_by_worktree_path("/tmp/worktrees/myapp/feat-a");
    assert!(found.is_some());
    assert_eq!(found.unwrap().name, "feat-a");
    let found = state.find_by_worktree_path("/tmp/worktrees/myapp/feat-a/src");
    assert!(found.is_some());
}
