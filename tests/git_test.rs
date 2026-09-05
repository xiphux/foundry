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
fn test_has_uncommitted_changes_dirty_untracked() {
    let repo = init_test_repo();
    std::fs::write(repo.path().join("file.txt"), "hello").unwrap();
    assert!(foundry::git::has_uncommitted_changes(repo.path()).unwrap());
}

#[test]
fn test_has_uncommitted_changes_dirty_modified() {
    let repo = init_test_repo();
    // Create and commit a tracked file, then modify it
    std::fs::write(repo.path().join("tracked.txt"), "original").unwrap();
    Command::new("git")
        .args(["add", "tracked.txt"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "add tracked"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    std::fs::write(repo.path().join("tracked.txt"), "modified").unwrap();
    assert!(foundry::git::has_uncommitted_changes(repo.path()).unwrap());
}

#[test]
fn test_has_modified_tracked_files_clean() {
    let repo = init_test_repo();
    assert!(!foundry::git::has_modified_tracked_files(repo.path()).unwrap());
}

#[test]
fn test_has_modified_tracked_files_ignores_untracked() {
    let repo = init_test_repo();
    std::fs::write(repo.path().join("untracked.txt"), "hello").unwrap();
    // Untracked files should NOT be flagged
    assert!(!foundry::git::has_modified_tracked_files(repo.path()).unwrap());
}

#[test]
fn test_has_modified_tracked_files_detects_modified() {
    let repo = init_test_repo();
    // Create and commit a tracked file, then modify it
    std::fs::write(repo.path().join("tracked.txt"), "original").unwrap();
    Command::new("git")
        .args(["add", "tracked.txt"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "add tracked"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    std::fs::write(repo.path().join("tracked.txt"), "modified").unwrap();
    // Modified tracked file should be flagged
    assert!(foundry::git::has_modified_tracked_files(repo.path()).unwrap());
}

#[test]
fn test_archive_branch_collision() {
    let repo = init_test_repo();

    foundry::git::create_branch(repo.path(), "feat").unwrap();
    let first = foundry::git::archive_branch(repo.path(), "feat", "archive").unwrap();

    foundry::git::create_branch(repo.path(), "feat").unwrap();
    let second = foundry::git::archive_branch(repo.path(), "feat", "archive").unwrap();

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

    // The reported name must be the branch that actually exists. `discard`
    // used to re-derive it as `{prefix}/{branch}-{YYYYMMDD}` and recorded a
    // nonexistent branch in the history log for the second archive of the day.
    assert_ne!(first, second);
    for name in [&first, &second] {
        assert!(
            foundry::git::branch_exists(repo.path(), name).unwrap(),
            "archive_branch reported '{name}', which does not exist:\n{branches}"
        );
    }
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

#[test]
fn test_log_commits() {
    let repo = init_test_repo();
    Command::new("git")
        .args(["checkout", "-b", "feature"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "first change"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "second change"])
        .current_dir(repo.path())
        .output()
        .unwrap();

    let log = foundry::git::log_commits(repo.path(), "main", "feature").unwrap();
    let lines: Vec<&str> = log.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("second change"));
    assert!(lines[1].contains("first change"));
}

#[test]
fn test_log_commits_no_commits() {
    let repo = init_test_repo();
    foundry::git::create_branch(repo.path(), "feature").unwrap();

    let log = foundry::git::log_commits(repo.path(), "main", "feature").unwrap();
    assert!(log.is_empty());
}

#[test]
fn test_status_porcelain() {
    let repo = init_test_repo();

    // Clean repo
    let status = foundry::git::status_porcelain(repo.path()).unwrap();
    assert!(status.is_empty());

    // Untracked file
    std::fs::write(repo.path().join("untracked.txt"), "data").unwrap();
    let status = foundry::git::status_porcelain(repo.path()).unwrap();
    assert!(status.contains("untracked.txt"));
}

#[test]
fn test_commit_count_no_commits() {
    let repo = init_test_repo();
    Command::new("git")
        .args(["branch", "feature"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    assert_eq!(
        foundry::git::commit_count(repo.path(), "feature", "main").unwrap(),
        0
    );
}

#[test]
fn test_commit_count_with_commits() {
    let repo = init_test_repo();
    Command::new("git")
        .args(["checkout", "-b", "feature"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    for msg in ["feat 1", "feat 2"] {
        Command::new("git")
            .args(["commit", "--allow-empty", "-m", msg])
            .current_dir(repo.path())
            .output()
            .unwrap();
    }
    assert_eq!(
        foundry::git::commit_count(repo.path(), "feature", "main").unwrap(),
        2
    );
}

/// Revision operands cannot be delimited with `--` (git would read them as
/// pathspecs), so they are rejected up front instead. A silent wrong answer is
/// the failure mode being avoided: `git log --oneline -- main..feature`
/// succeeds and reports nothing.
#[test]
fn test_revision_operands_reject_leading_hyphen() {
    let repo = init_test_repo();

    assert!(foundry::git::merge(repo.path(), "--exec=touch /tmp/x").is_err());
    assert!(foundry::git::merge_ff_only(repo.path(), "-x").is_err());
    assert!(foundry::git::commit_count(repo.path(), "-x", "main").is_err());
    assert!(foundry::git::commit_count(repo.path(), "main", "-x").is_err());
    assert!(foundry::git::log_commits(repo.path(), "-x", "main").is_err());
    assert!(foundry::git::stream_diff_committed(repo.path(), "-x", "main", true).is_err());
    assert!(foundry::git::push_branch(repo.path(), "-x", "main").is_err());
}

/// The guard must not reject ordinary refs.
#[test]
fn test_revision_operands_allow_normal_refs() {
    let repo = init_test_repo();
    // Same ref both sides: zero commits, but importantly not an error.
    assert_eq!(
        foundry::git::commit_count(repo.path(), "main", "main").unwrap(),
        0
    );
    assert!(foundry::git::log_commits(repo.path(), "main", "main").is_ok());
}

/// Operands that do take `--` must still work for names containing characters
/// git would otherwise have to guess about.
#[test]
fn test_branch_operations_survive_the_end_of_options_delimiter() {
    let repo = init_test_repo();

    foundry::git::create_branch(repo.path(), "feature/x").unwrap();
    assert!(foundry::git::branch_exists(repo.path(), "feature/x").unwrap());

    foundry::git::archive_branch(repo.path(), "feature/x", "archive").unwrap();
    assert!(!foundry::git::branch_exists(repo.path(), "feature/x").unwrap());

    let archived = foundry::git::list_branches_with_prefix(repo.path(), "archive/").unwrap();
    assert_eq!(archived.len(), 1, "got {archived:?}");

    foundry::git::delete_branch(repo.path(), &archived[0]).unwrap();
    assert!(
        foundry::git::list_branches_with_prefix(repo.path(), "archive/")
            .unwrap()
            .is_empty()
    );
}

/// Inside a linked worktree, `repo_root` reports the worktree itself. Anything
/// that identifies a *project* has to resolve back to the source repo instead —
/// `resolve_project` used the former and auto-registered a workspace's worktree
/// as a new project, then created worktrees of it.
#[test]
fn test_main_repo_root_resolves_back_from_a_linked_worktree() {
    let repo = init_test_repo();
    let wt_parent = TempDir::new().unwrap();
    let worktree = wt_parent.path().join("feature");

    Command::new("git")
        .args([
            "worktree",
            "add",
            worktree.to_str().unwrap(),
            "-b",
            "feature",
        ])
        .current_dir(repo.path())
        .output()
        .unwrap();
    assert!(worktree.exists(), "worktree was not created");

    let canonical_repo = std::fs::canonicalize(repo.path()).unwrap();

    // The worktree reports itself...
    assert_eq!(
        std::fs::canonicalize(foundry::git::repo_root(&worktree).unwrap()).unwrap(),
        std::fs::canonicalize(&worktree).unwrap()
    );
    // ...while the project resolution reaches the source repo.
    assert_eq!(
        std::fs::canonicalize(foundry::git::main_repo_root(&worktree).unwrap()).unwrap(),
        canonical_repo
    );

    // And it stays correct when already in the main repo.
    assert_eq!(
        std::fs::canonicalize(foundry::git::main_repo_root(repo.path()).unwrap()).unwrap(),
        canonical_repo
    );
}

#[test]
fn test_worktree_registered_tracks_add_and_remove() {
    let repo = init_test_repo();
    let worktree = repo.path().join("wt");

    assert!(!foundry::git::worktree_registered(repo.path(), &worktree).unwrap());

    foundry::git::create_branch(repo.path(), "feat").unwrap();
    foundry::git::create_worktree(repo.path(), &worktree, "feat").unwrap();
    assert!(foundry::git::worktree_registered(repo.path(), &worktree).unwrap());

    foundry::git::remove_worktree(repo.path(), &worktree, false).unwrap();
    assert!(!foundry::git::worktree_registered(repo.path(), &worktree).unwrap());
}

/// Git lists the fully resolved path, so a worktree reached through a symlinked
/// parent is spelled one way by git and another by foundry. These tests live in
/// exactly such a path: `TempDir` sits under `std::env::temp_dir()`, which on
/// macOS is `$TMPDIR` beneath `/var/folders/...`, reached through the `/var` ->
/// `/private/var` symlink. Both spellings have to be recognised as the same
/// worktree, or a partial removal reads as a refusal.
#[cfg(unix)]
#[test]
fn test_worktree_registered_matches_a_path_spelled_through_a_symlink() {
    let repo = init_test_repo();
    let real = repo.path().join("real");
    std::fs::create_dir(&real).unwrap();
    std::os::unix::fs::symlink(&real, repo.path().join("link")).unwrap();

    foundry::git::create_branch(repo.path(), "feat").unwrap();
    foundry::git::create_worktree(repo.path(), &real.join("wt"), "feat").unwrap();

    assert!(
        foundry::git::worktree_registered(repo.path(), &repo.path().join("link").join("wt"))
            .unwrap()
    );
}
