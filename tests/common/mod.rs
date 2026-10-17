//! Helpers shared by the integration tests.
//!
//! Each test file compiles as its own crate and uses a different subset of
//! these, so any one crate sees the rest as unused.
#![allow(dead_code)]

use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

/// Commit identity, passed per command so tests never depend on — or touch —
/// the developer's git config.
const GIT_IDENTITY: [(&str, &str); 4] = [
    ("GIT_AUTHOR_NAME", "Foundry Test"),
    ("GIT_AUTHOR_EMAIL", "test@foundry.invalid"),
    ("GIT_COMMITTER_NAME", "Foundry Test"),
    ("GIT_COMMITTER_EMAIL", "test@foundry.invalid"),
];

/// Run git in `dir`, failing the test with git's stderr if it fails, and
/// return stdout.
///
/// Asserting here is the point: a setup step that failed silently surfaced
/// later as a confusing assertion about something else.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .envs(GIT_IDENTITY)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Make `dir` a repository on `main` with one empty commit.
pub fn init_repo_at(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main"]);
    git(dir, &["commit", "-q", "--allow-empty", "-m", "initial"]);
}

/// A new temporary repository on `main` with one empty commit.
pub fn init_repo() -> TempDir {
    let dir = TempDir::new().unwrap();
    init_repo_at(dir.path());
    dir
}
