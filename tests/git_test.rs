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

#[test]
fn test_branch_has_commits_true() {
    let repo = init_test_repo();
    Command::new("git")
        .args(["checkout", "-b", "feature"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "feature work"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    assert!(foundry::git::branch_has_commits(repo.path(), "feature", "main").unwrap());
}

#[test]
fn test_branch_has_commits_false() {
    let repo = init_test_repo();
    foundry::git::create_branch(repo.path(), "feature").unwrap();
    assert!(!foundry::git::branch_has_commits(repo.path(), "feature", "main").unwrap());
}

#[test]
fn test_branch_exists_true() {
    let repo = init_test_repo();
    foundry::git::create_branch(repo.path(), "my-branch").unwrap();
    assert!(foundry::git::branch_exists(repo.path(), "my-branch").unwrap());
}

#[test]
fn test_branch_exists_false() {
    let repo = init_test_repo();
    assert!(!foundry::git::branch_exists(repo.path(), "nonexistent").unwrap());
}

#[test]
fn test_current_branch() {
    let repo = init_test_repo();
    let branch = foundry::git::current_branch(repo.path()).unwrap();
    assert_eq!(branch, "main");
}

#[test]
fn test_delete_branch() {
    let repo = init_test_repo();
    foundry::git::create_branch(repo.path(), "to-delete").unwrap();
    assert!(foundry::git::branch_exists(repo.path(), "to-delete").unwrap());
    foundry::git::delete_branch(repo.path(), "to-delete").unwrap();
    assert!(!foundry::git::branch_exists(repo.path(), "to-delete").unwrap());
}

#[test]
fn test_list_branches_with_prefix_matching() {
    let repo = init_test_repo();
    foundry::git::create_branch(repo.path(), "feature/one").unwrap();
    foundry::git::create_branch(repo.path(), "feature/two").unwrap();
    foundry::git::create_branch(repo.path(), "bugfix/one").unwrap();
    let branches = foundry::git::list_branches_with_prefix(repo.path(), "feature/").unwrap();
    assert_eq!(branches.len(), 2);
    assert!(branches.contains(&"feature/one".to_string()));
    assert!(branches.contains(&"feature/two".to_string()));
}

#[test]
fn test_list_branches_with_prefix_no_match() {
    let repo = init_test_repo();
    foundry::git::create_branch(repo.path(), "feature/one").unwrap();
    let branches = foundry::git::list_branches_with_prefix(repo.path(), "archive/").unwrap();
    assert!(branches.is_empty());
}

#[test]
fn test_merge_non_ff() {
    let repo = init_test_repo();
    // Create a feature branch with a commit
    Command::new("git")
        .args(["checkout", "-b", "feature"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    std::fs::write(repo.path().join("feature.txt"), "feature content").unwrap();
    Command::new("git")
        .args(["add", "feature.txt"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "feature commit"])
        .current_dir(repo.path())
        .output()
        .unwrap();

    // Go back to main and add a diverging commit
    Command::new("git")
        .args(["checkout", "main"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    std::fs::write(repo.path().join("main.txt"), "main content").unwrap();
    Command::new("git")
        .args(["add", "main.txt"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "main commit"])
        .current_dir(repo.path())
        .output()
        .unwrap();

    // Merge feature into main (non-ff)
    foundry::git::merge(repo.path(), "feature").unwrap();

    // Verify both files exist after merge
    assert!(repo.path().join("feature.txt").exists());
    assert!(repo.path().join("main.txt").exists());
}
