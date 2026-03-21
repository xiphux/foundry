use std::process::Command;
use tempfile::TempDir;

fn init_test_repo() -> TempDir {
    let dir = TempDir::new().unwrap();
    Command::new("git")
        .args(["init"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "initial"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["branch", "-M", "main"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    dir
}

#[test]
fn test_detect_main_branch() {
    let repo = init_test_repo();
    let branch = foundry::git::detect_main_branch(repo.path()).unwrap();
    assert_eq!(branch, "main");
}

#[test]
fn test_detect_master_branch() {
    let repo = init_test_repo();
    Command::new("git")
        .args(["branch", "-M", "master"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    let branch = foundry::git::detect_main_branch(repo.path()).unwrap();
    assert_eq!(branch, "master");
}

#[test]
fn test_create_branch() {
    let repo = init_test_repo();
    foundry::git::create_branch(repo.path(), "feat/test").unwrap();
    let output = Command::new("git")
        .args(["branch", "--list", "feat/test"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("feat/test"));
}

#[test]
fn test_has_uncommitted_changes_clean() {
    let repo = init_test_repo();
    assert!(!foundry::git::has_uncommitted_changes(repo.path()).unwrap());
}

#[test]
fn test_has_uncommitted_changes_dirty() {
    let repo = init_test_repo();
    std::fs::write(repo.path().join("file.txt"), "hello").unwrap();
    assert!(foundry::git::has_uncommitted_changes(repo.path()).unwrap());
}

#[test]
fn test_archive_branch_collision() {
    let repo = init_test_repo();

    foundry::git::create_branch(repo.path(), "feat").unwrap();
    foundry::git::archive_branch(repo.path(), "feat", "archive").unwrap();

    foundry::git::create_branch(repo.path(), "feat").unwrap();
    foundry::git::archive_branch(repo.path(), "feat", "archive").unwrap();

    let output = Command::new("git")
        .args(["branch", "--list", "archive/feat-*"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    let branches = String::from_utf8_lossy(&output.stdout);
    let count = branches.lines().filter(|l| !l.trim().is_empty()).count();
    assert!(
        count >= 2,
        "expected at least 2 archived branches, got {count}: {branches}"
    );
}
