mod common;
use common::*;
use tempfile::TempDir;

#[test]
fn test_git_workflow_start_to_finish() {
    let repo_dir = TempDir::new().unwrap();
    let worktree_base = TempDir::new().unwrap();
    init_repo_at(repo_dir.path());

    let source = repo_dir.path();
    let worktree_path = worktree_base.path().join("myapp").join("my-feature");

    // Create branch and worktree
    foundry::git::create_branch(source, "xiphux/my-feature").unwrap();
    std::fs::create_dir_all(worktree_path.parent().unwrap()).unwrap();
    foundry::git::create_worktree(source, &worktree_path, "xiphux/my-feature").unwrap();

    // Make a commit in the worktree
    std::fs::write(worktree_path.join("feature.txt"), "hello").unwrap();
    git(&worktree_path, &["add", "."]);
    git(&worktree_path, &["commit", "-m", "add feature"]);

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
    let output = git(source, &["log", "--oneline", "-1"]);
    let log = output;
    assert!(log.contains("add feature"));

    // Verify: branch should be archived
    let output = git(source, &["branch", "--list", "archive/xiphux/my-feature-*"]);
    let branches = output;
    assert!(
        !branches.trim().is_empty(),
        "expected archived branch, got empty"
    );
}
