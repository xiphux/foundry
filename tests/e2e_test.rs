//! End-to-end: the real binary, run against a throwaway repo and foundry home.
//!
//! The workflow modules' building blocks have unit tests, but nothing else
//! runs a command from argument parsing through git, state, history and
//! cleanup in the order the command does them. `FOUNDRY_HOME` keeps the run
//! away from the real ~/.foundry, and `FOUNDRY_TERMINAL=bare` keeps it from
//! opening tabs or tmux sessions: the pane command runs in the foreground and
//! returns.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

struct Sandbox {
    /// `FOUNDRY_HOME`.
    home: TempDir,
    /// Holds the repository, named `myapp` so the auto-registered project is.
    _repo_parent: TempDir,
    repo: PathBuf,
    worktrees: TempDir,
}

const GIT_IDENTITY: [(&str, &str); 4] = [
    ("GIT_AUTHOR_NAME", "Foundry Test"),
    ("GIT_AUTHOR_EMAIL", "test@foundry.invalid"),
    ("GIT_COMMITTER_NAME", "Foundry Test"),
    ("GIT_COMMITTER_EMAIL", "test@foundry.invalid"),
];

fn git(dir: &Path, args: &[&str]) -> String {
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

/// A TOML literal string, so Windows backslashes need no escaping.
fn toml_path(path: &Path) -> String {
    format!("'{}'", path.display())
}

impl Sandbox {
    /// A repo on `main` with one commit, and a global config whose only pane
    /// records that it ran by touching `pane-ran` in the foundry home.
    fn new() -> Self {
        let home = TempDir::new().unwrap();
        let repo_parent = TempDir::new().unwrap();
        let worktrees = TempDir::new().unwrap();
        let repo = repo_parent.path().join("myapp");
        std::fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("README.md"), "myapp\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "initial"]);

        std::fs::write(
            home.path().join("config.toml"),
            format!(
                "worktree_dir = {}\n\n[[panes]]\nname = \"main\"\ncommand = 'echo {{name}} >> \"$FOUNDRY_HOME/pane-ran\"'\n",
                toml_path(worktrees.path())
            ),
        )
        .unwrap();

        Self {
            home,
            _repo_parent: repo_parent,
            repo,
            worktrees,
        }
    }

    fn foundry(&self, args: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_foundry"));
        command
            .args(args)
            .current_dir(&self.repo)
            .env("FOUNDRY_HOME", self.home.path())
            .env("FOUNDRY_TERMINAL", "bare")
            .envs(GIT_IDENTITY);
        // Nothing from the terminal running the tests may leak into detection.
        for var in ["TERM_PROGRAM", "WT_SESSION", "TMUX", "ZELLIJ"] {
            command.env_remove(var);
        }
        command.output().unwrap()
    }

    /// Run foundry, fail the test with its output if it fails, return stdout.
    fn ok(&self, args: &[&str]) -> String {
        let output = self.foundry(args);
        assert!(
            output.status.success(),
            "foundry {args:?} failed\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    /// Run foundry, expect failure, return stderr.
    fn fails(&self, args: &[&str]) -> String {
        let output = self.foundry(args);
        assert!(
            !output.status.success(),
            "foundry {args:?} should have failed\nstdout: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        String::from_utf8_lossy(&output.stderr).into_owned()
    }

    fn worktree(&self, name: &str) -> PathBuf {
        self.worktrees.path().join("myapp").join(name)
    }

    fn home_file(&self, name: &str) -> String {
        std::fs::read_to_string(self.home.path().join(name)).unwrap_or_default()
    }

    fn state(&self) -> foundry::state::WorkspaceState {
        foundry::state::WorkspaceState::load_from(&self.home.path().join("state.toml")).unwrap()
    }

    fn history(&self) -> Vec<foundry::history::HistoryEvent> {
        self.home_file("history.jsonl")
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn branches(&self) -> Vec<String> {
        git(&self.repo, &["branch", "--format=%(refname:short)"])
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn registered_worktrees(&self) -> usize {
        git(&self.repo, &["worktree", "list", "--porcelain"])
            .lines()
            .filter(|l| l.starts_with("worktree "))
            .count()
    }

    fn commit_in(&self, name: &str, file: &str) {
        let worktree = self.worktree(name);
        std::fs::write(worktree.join(file), "work\n").unwrap();
        git(&worktree, &["add", file]);
        git(&worktree, &["commit", "-q", "-m", &format!("add {file}")]);
    }
}

#[test]
fn start_creates_the_workspace_and_records_it() {
    let sb = Sandbox::new();
    sb.ok(&["start", "feat"]);

    let worktree = sb.worktree("feat");
    assert!(worktree.join("README.md").exists());
    assert_eq!(git(&worktree, &["branch", "--show-current"]).trim(), "feat");
    assert_eq!(sb.registered_worktrees(), 2);

    let state = sb.state();
    let ws = state.find("myapp", "feat").expect("workspace recorded");
    assert_eq!(ws.branch, "feat");
    // Bare mode's tab id is the worktree path; it being set shows the
    // workspace was opened and the id persisted afterwards.
    assert!(!ws.terminal_tab_id.is_empty());

    assert!(sb.home_file("projects.toml").contains("[projects.myapp]"));
    assert_eq!(sb.home_file("pane-ran").trim(), "feat");
    let events = sb.history();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event, "started");

    let list = sb.ok(&["list"]);
    assert!(list.contains("myapp/feat (branch: feat"), "{list}");
    let status = sb.ok(&["status"]);
    assert!(status.contains("feat"), "{status}");
}

#[test]
fn start_applies_the_branch_prefix_and_rejects_a_bad_name() {
    let sb = Sandbox::new();
    let config = sb.home.path().join("config.toml");
    let contents = std::fs::read_to_string(&config).unwrap();
    std::fs::write(&config, format!("branch_prefix = \"dev\"\n{contents}")).unwrap();

    sb.ok(&["start", "feat"]);
    assert!(sb.branches().contains(&"dev/feat".to_string()));
    assert_eq!(sb.state().find("myapp", "feat").unwrap().branch, "dev/feat");

    sb.fails(&["start", "../escape"]);
    assert_eq!(sb.state().list().len(), 1);
}

/// Finish merges into main, archives the branch (it has commits — merging
/// must not make it look commitless and get it deleted), and removes the
/// worktree, its registration and its state entry.
#[test]
fn finish_merges_and_archives_a_branch_with_commits() {
    let sb = Sandbox::new();
    sb.ok(&["start", "feat"]);
    sb.commit_in("feat", "feature.txt");

    sb.ok(&["finish", "feat"]);

    assert!(sb.repo.join("feature.txt").exists(), "merged into main");
    assert!(!sb.worktree("feat").exists());
    assert_eq!(sb.registered_worktrees(), 1);
    assert!(sb.state().list().is_empty());

    let branches = sb.branches();
    assert!(!branches.contains(&"feat".to_string()), "{branches:?}");
    let archived = branches
        .iter()
        .find(|b| b.starts_with("archive/feat-"))
        .unwrap_or_else(|| panic!("no archive branch in {branches:?}"));

    let finished = sb.history().pop().unwrap();
    assert_eq!(finished.event, "finished");
    assert_eq!(finished.commits, Some(1));
    assert_eq!(finished.merge_strategy.as_deref(), Some("ff-only"));
    assert!(archived.len() > "archive/feat-".len());
}

#[test]
fn finish_deletes_a_branch_with_no_commits() {
    let sb = Sandbox::new();
    sb.ok(&["start", "empty"]);
    sb.ok(&["finish", "empty"]);

    assert_eq!(sb.branches(), ["main"]);
    assert!(sb.state().list().is_empty());
    assert_eq!(sb.history().pop().unwrap().commits, Some(0));
}

#[test]
fn finish_refuses_uncommitted_work_and_changes_nothing() {
    let sb = Sandbox::new();
    sb.ok(&["start", "feat"]);
    std::fs::write(sb.worktree("feat").join("wip.txt"), "unsaved\n").unwrap();

    let stderr = sb.fails(&["finish", "feat"]);
    assert!(stderr.contains("uncommitted changes"), "{stderr}");
    assert!(sb.worktree("feat").join("wip.txt").exists());
    assert!(sb.state().find("myapp", "feat").is_some());
    assert!(sb.branches().contains(&"feat".to_string()));
}

#[test]
fn finish_with_merge_strategy_creates_a_merge_commit() {
    let sb = Sandbox::new();
    let config = sb.home.path().join("config.toml");
    let contents = std::fs::read_to_string(&config).unwrap();
    std::fs::write(&config, format!("merge_strategy = \"merge\"\n{contents}")).unwrap();

    sb.ok(&["start", "feat"]);
    sb.commit_in("feat", "feature.txt");
    // Move main on, so a fast-forward is impossible and a real merge is made.
    std::fs::write(sb.repo.join("main.txt"), "main\n").unwrap();
    git(&sb.repo, &["add", "main.txt"]);
    git(&sb.repo, &["commit", "-q", "-m", "main moves"]);

    sb.ok(&["finish", "feat"]);
    assert!(sb.repo.join("feature.txt").exists());
    let parents = git(&sb.repo, &["log", "-1", "--format=%P"]);
    assert_eq!(parents.split_whitespace().count(), 2, "HEAD is a merge");
    assert_eq!(
        sb.history().pop().unwrap().merge_strategy.as_deref(),
        Some("merge")
    );
}

#[test]
fn discard_needs_force_for_unmerged_commits_then_archives() {
    let sb = Sandbox::new();
    sb.ok(&["start", "feat"]);
    sb.commit_in("feat", "feature.txt");

    let stderr = sb.fails(&["discard", "feat"]);
    assert!(stderr.contains("1 unmerged commit"), "{stderr}");
    assert!(sb.worktree("feat").exists());

    sb.ok(&["discard", "feat", "--force"]);
    assert!(!sb.worktree("feat").exists());
    assert!(!sb.repo.join("feature.txt").exists(), "not merged");
    assert_eq!(sb.registered_worktrees(), 1);
    assert!(sb.state().list().is_empty());
    assert!(sb.branches().iter().any(|b| b.starts_with("archive/feat-")));

    let discarded = sb.history().pop().unwrap();
    assert_eq!(discarded.event, "discarded");
    assert!(discarded.archived_as.unwrap().starts_with("archive/feat-"));
}

#[test]
fn discard_without_commits_deletes_the_branch() {
    let sb = Sandbox::new();
    sb.ok(&["start", "scratch"]);
    // Uncommitted work alone does not need --force, and --yes skips the prompt.
    std::fs::write(sb.worktree("scratch").join("junk.txt"), "x\n").unwrap();
    sb.ok(&["discard", "scratch", "--yes"]);

    assert_eq!(sb.branches(), ["main"]);
    assert!(sb.state().list().is_empty());
    assert_eq!(sb.history().pop().unwrap().archived_as, None);
}

/// A workspace whose directory vanished is hidden from `list` but kept in
/// state, because `discard` is the only thing that can still clear its
/// branch and worktree registration.
#[test]
fn discard_clears_a_workspace_whose_directory_is_gone() {
    let sb = Sandbox::new();
    sb.ok(&["start", "gone"]);
    std::fs::remove_dir_all(sb.worktree("gone")).unwrap();

    assert!(sb.ok(&["list"]).contains("No active workspaces"));
    assert!(
        sb.state().find("myapp", "gone").is_some(),
        "list must not prune state"
    );

    sb.ok(&["discard", "gone"]);
    assert!(sb.state().list().is_empty());
    assert_eq!(sb.branches(), ["main"]);
    assert_eq!(sb.registered_worktrees(), 1);
}

#[test]
fn restore_recreates_a_workspace_from_its_archive() {
    let sb = Sandbox::new();
    sb.ok(&["start", "feat"]);
    sb.commit_in("feat", "feature.txt");
    sb.ok(&["discard", "feat", "--force"]);
    let archived = sb
        .branches()
        .into_iter()
        .find(|b| b.starts_with("archive/feat-"))
        .unwrap();

    let listing = sb.ok(&["restore"]);
    assert!(listing.contains(&archived), "{listing}");

    // Accepts the name without the archive prefix.
    sb.ok(&["restore", archived.trim_start_matches("archive/")]);

    let worktree = sb.worktree("feat");
    assert!(
        worktree.join("feature.txt").exists(),
        "archived work is back"
    );
    let ws = sb.state().find("myapp", "feat").cloned().expect("restored");
    assert_eq!(ws.branch, archived);
    let restored = sb.history().pop().unwrap();
    assert_eq!(restored.event, "restored");
    assert_eq!(restored.from_branch.as_deref(), Some(archived.as_str()));
}

#[test]
fn open_reopens_an_existing_workspace() {
    let sb = Sandbox::new();
    sb.ok(&["start", "feat"]);
    sb.ok(&["open", "feat"]);
    assert_eq!(sb.home_file("pane-ran").lines().count(), 2);

    let stderr = sb.fails(&["open", "missing"]);
    assert!(stderr.contains("missing"), "{stderr}");
}

#[test]
fn diff_shows_the_workspace_changes_against_main() {
    let sb = Sandbox::new();
    sb.ok(&["start", "feat"]);
    sb.commit_in("feat", "feature.txt");

    let stat = sb.ok(&["diff", "feat", "--stat"]);
    assert!(stat.contains("feature.txt"), "{stat}");
    let full = sb.ok(&["diff", "feat"]);
    assert!(full.contains("+work"), "{full}");
}

/// Setup scripts run before the workspace opens with the allocated ports
/// exported, teardown scripts run on discard, and neither runs at all until
/// the project config is approved.
#[test]
fn project_scripts_run_only_once_the_config_is_trusted() {
    let sb = Sandbox::new();
    let project_config = "ports = [\"APP_PORT\"]\n\n\
         [[scripts.setup]]\n\
         name = \"record setup\"\n\
         command = 'echo \"{name} $APP_PORT\" >> \"$FOUNDRY_HOME/setup-ran\"'\n\n\
         [[scripts.teardown]]\n\
         name = \"record teardown\"\n\
         command = 'echo {branch} >> \"$FOUNDRY_HOME/teardown-ran\"'\n";
    std::fs::write(sb.repo.join(".foundry.toml"), project_config).unwrap();
    git(&sb.repo, &["add", ".foundry.toml"]);
    git(&sb.repo, &["commit", "-q", "-m", "project config"]);

    // stdin is not a terminal here, so an unapproved config is refused
    // outright, before any branch or worktree exists.
    let stderr = sb.fails(&["start", "feat"]);
    assert!(stderr.contains("has not been approved"), "{stderr}");
    assert_eq!(sb.branches(), ["main"]);
    assert_eq!(sb.home_file("setup-ran"), "");

    let mut store =
        foundry::trust::TrustStore::load_from(&sb.home.path().join("trust.toml")).unwrap();
    store.trust(&sb.repo, &foundry::trust::hash_config(project_config));
    store.save_to(&sb.home.path().join("trust.toml")).unwrap();

    sb.ok(&["start", "feat"]);
    let port = sb.state().find("myapp", "feat").unwrap().allocated_ports["APP_PORT"];
    assert_eq!(sb.home_file("setup-ran").trim(), format!("feat {port}"));

    sb.ok(&["discard", "feat"]);
    assert_eq!(sb.home_file("teardown-ran").trim(), "feat");
}

#[test]
fn projects_commands_manage_the_registry() {
    let sb = Sandbox::new();
    assert!(
        sb.ok(&["projects", "list"])
            .contains("No registered projects")
    );

    let repo = sb.repo.to_string_lossy().into_owned();
    sb.ok(&["projects", "add", "renamed", &repo]);
    let list = sb.ok(&["projects", "list"]);
    assert!(list.contains("renamed:"), "{list}");

    // Registered under a custom name, the project is found by path.
    sb.ok(&["start", "feat"]);
    assert!(sb.state().find("renamed", "feat").is_some());

    sb.ok(&["discard", "feat"]);
    sb.ok(&["projects", "remove", "renamed"]);
    assert!(
        sb.ok(&["projects", "list"])
            .contains("No registered projects")
    );
}
