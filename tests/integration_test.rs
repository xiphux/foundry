use std::process::Command;
use tempfile::TempDir;

fn init_test_repo(dir: &std::path::Path) {
    Command::new("git")
        .args(["init"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "initial"])
        .current_dir(dir)
        .output()
        .unwrap();
    Command::new("git")
        .args(["branch", "-M", "main"])
        .current_dir(dir)
        .output()
        .unwrap();
}

#[test]
fn test_git_workflow_start_to_finish() {
    let repo_dir = TempDir::new().unwrap();
    let worktree_base = TempDir::new().unwrap();
    init_test_repo(repo_dir.path());

    let source = repo_dir.path();
    let worktree_path = worktree_base.path().join("myapp").join("my-feature");

    // Create branch and worktree
    foundry::git::create_branch(source, "xiphux/my-feature").unwrap();
    std::fs::create_dir_all(worktree_path.parent().unwrap()).unwrap();
    foundry::git::create_worktree(source, &worktree_path, "xiphux/my-feature").unwrap();

    // Make a commit in the worktree
    std::fs::write(worktree_path.join("feature.txt"), "hello").unwrap();
    Command::new("git")
        .args(["add", "."])
        .current_dir(&worktree_path)
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "add feature"])
        .current_dir(&worktree_path)
        .output()
        .unwrap();

    // Verify no uncommitted changes
    assert!(!foundry::git::has_uncommitted_changes(&worktree_path).unwrap());
    assert!(!foundry::git::has_uncommitted_changes(source).unwrap());

    // Merge ff-only
    foundry::git::merge_ff_only(source, "xiphux/my-feature").unwrap();

    // Remove worktree
    foundry::git::remove_worktree(source, &worktree_path, false).unwrap();

    // Archive branch
    foundry::git::archive_branch(source, "xiphux/my-feature", "archive").unwrap();

    // Verify: feature.txt should be in main now
    let output = Command::new("git")
        .args(["log", "--oneline", "-1"])
        .current_dir(source)
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&output.stdout);
    assert!(log.contains("add feature"));

    // Verify: branch should be archived
    let output = Command::new("git")
        .args(["branch", "--list", "archive/xiphux/my-feature-*"])
        .current_dir(source)
        .output()
        .unwrap();
    let branches = String::from_utf8_lossy(&output.stdout);
    assert!(
        !branches.trim().is_empty(),
        "expected archived branch, got empty"
    );
}
