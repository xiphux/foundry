use std::process::Command;
use tempfile::TempDir;

fn init_test_repo() -> TempDir {
    let dir = TempDir::new().unwrap();
    Command::new("git")
        .args(["init", "--initial-branch=main"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    // Configure git identity for CI environments without global config
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "initial"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    dir
}

#[test]
fn test_list_remotes_none() {
    let dir = init_test_repo();
    let remotes = foundry::git::list_remotes(dir.path()).unwrap();
    assert!(remotes.is_empty());
}

#[test]
fn test_list_remotes_single() {
    let dir = init_test_repo();
    Command::new("git")
        .args([
            "remote",
            "add",
            "origin",
            "https://github.com/user/repo.git",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let remotes = foundry::git::list_remotes(dir.path()).unwrap();
    assert_eq!(remotes, vec!["origin"]);
}

#[test]
fn test_list_remotes_multiple() {
    let dir = init_test_repo();
    Command::new("git")
        .args([
            "remote",
            "add",
            "origin",
            "https://github.com/user/repo.git",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args([
            "remote",
            "add",
            "upstream",
            "https://github.com/org/repo.git",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let mut remotes = foundry::git::list_remotes(dir.path()).unwrap();
    remotes.sort();
    assert_eq!(remotes, vec!["origin", "upstream"]);
}

#[test]
fn test_remote_url() {
    let dir = init_test_repo();
    Command::new("git")
        .args([
            "remote",
            "add",
            "origin",
            "https://github.com/user/repo.git",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let url = foundry::git::remote_url(dir.path(), "origin").unwrap();
    assert_eq!(url, "https://github.com/user/repo.git");
}

#[test]
fn test_push_branch_no_remote() {
    let dir = init_test_repo();
    // push_branch should fail when no remote exists
    let result = foundry::git::push_branch(dir.path(), "origin", "main");
    assert!(result.is_err());
}

#[test]
fn test_detect_forge_github_https() {
    assert!(matches!(
        foundry::forge::detect_forge_from_url("https://github.com/user/repo.git"),
        Some(foundry::forge::ForgeKind::GitHub)
    ));
}

#[test]
fn test_detect_forge_github_ssh() {
    assert!(matches!(
        foundry::forge::detect_forge_from_url("git@github.com:user/repo.git"),
        Some(foundry::forge::ForgeKind::GitHub)
    ));
}

#[test]
fn test_detect_forge_gitlab_https() {
    assert!(matches!(
        foundry::forge::detect_forge_from_url("https://gitlab.com/user/repo.git"),
        Some(foundry::forge::ForgeKind::GitLab)
    ));
}

#[test]
fn test_detect_forge_unknown() {
    assert!(foundry::forge::detect_forge_from_url("https://bitbucket.org/user/repo.git").is_none());
}

#[test]
fn test_resolve_pr_remote_single() {
    let remote = foundry::forge::resolve_pr_remote(None, &["upstream".to_string()]);
    assert_eq!(remote, "upstream");
}

#[test]
fn test_resolve_pr_remote_multiple_defaults_to_origin() {
    let remote = foundry::forge::resolve_pr_remote(None, &["origin".into(), "upstream".into()]);
    assert_eq!(remote, "origin");
}

#[test]
fn test_resolve_pr_remote_explicit_config() {
    let remote =
        foundry::forge::resolve_pr_remote(Some("upstream"), &["origin".into(), "upstream".into()]);
    assert_eq!(remote, "upstream");
}

#[test]
fn test_resolve_pr_remote_empty_defaults_to_origin() {
    let remote = foundry::forge::resolve_pr_remote(None, &[]);
    assert_eq!(remote, "origin");
}

#[test]
fn test_detect_forge_with_github_remote() {
    let dir = init_test_repo();
    Command::new("git")
        .args([
            "remote",
            "add",
            "origin",
            "https://github.com/user/repo.git",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let result = foundry::forge::detect_forge(dir.path(), None);
    assert!(result.is_ok());
    let (_forge, remote) = result.unwrap();
    assert_eq!(remote, "origin");
}

#[test]
fn test_detect_forge_with_unknown_remote() {
    let dir = init_test_repo();
    Command::new("git")
        .args([
            "remote",
            "add",
            "origin",
            "https://bitbucket.org/user/repo.git",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let result = foundry::forge::detect_forge(dir.path(), None);
    assert!(result.is_err());
    let err = result.err().unwrap().to_string();
    assert!(
        err.contains("could not detect forge"),
        "Error should mention forge detection: {err}"
    );
}

#[test]
fn test_detect_forge_no_remotes() {
    let dir = init_test_repo();

    let result = foundry::forge::detect_forge(dir.path(), None);
    assert!(result.is_err());
    let err = result.err().unwrap().to_string();
    assert!(
        err.contains("not found"),
        "Error should mention remote not found: {err}"
    );
}

#[test]
fn test_detect_forge_configured_remote() {
    let dir = init_test_repo();
    Command::new("git")
        .args([
            "remote",
            "add",
            "origin",
            "https://bitbucket.org/user/repo.git",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args([
            "remote",
            "add",
            "github",
            "https://github.com/user/repo.git",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Without config, defaults to "origin" (bitbucket) — should fail
    let result = foundry::forge::detect_forge(dir.path(), None);
    assert!(result.is_err());

    // With config pointing to "github" remote — should succeed
    let result = foundry::forge::detect_forge(dir.path(), Some("github"));
    assert!(result.is_ok());
    let (_forge, remote) = result.unwrap();
    assert_eq!(remote, "github");
}
