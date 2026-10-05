//! The dependency audit fails a change only for the RustSec findings it adds
//! over a baseline commit, so what decides the baseline
//! (scripts/audit-baseline.sh) and what counts as added
//! (scripts/audit-new-advisories.sh) are the whole of the gate. An earlier
//! version compared a push only with its parent, and a pull request with the
//! target branch's tip; both let a finding through, and both are pinned here.
//!
//! The scripts are driven end to end against throwaway repositories, with
//! fakes first on PATH: `gh` serves canned run lists per branch, and `cargo
//! audit --json --file <lock>` prints the lock itself -- each commit's
//! Cargo.lock here *is* the report its audit would produce, so the baseline's
//! copy reports its own. They are bash scripts that only ever run on Linux
//! CI, so these cases are Unix-only; the wiring checks at the bottom run
//! everywhere.
//!
//! Separately, ci.yml has to give the baseline step the full history,
//! `actions: read`, and workflows that exist, and hand the audit what it
//! found. None of those fails visibly when missing -- a dropped AUDIT_BASE
//! quietly falls back to comparing against HEAD's parent -- so they are
//! pinned too.

use std::path::{Path, PathBuf};

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn read(relative: &str) -> String {
    let path = root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

#[cfg(unix)]
mod scripts {
    use super::*;
    use std::collections::HashMap;
    use std::os::unix::fs::PermissionsExt;
    use std::process::{Command, Output};
    use tempfile::TempDir;

    struct Fixture {
        _scratch: TempDir,
        bin: PathBuf,
        scratch: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let scratch = tempfile::tempdir().unwrap();
            let bin = scratch.path().join("bin");
            std::fs::create_dir(&bin).unwrap();
            let fake = |name: &str, body: &str| {
                let path = bin.join(name);
                std::fs::write(&path, format!("#!/usr/bin/env bash\n{body}\n")).unwrap();
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            };
            // gh api ... -f branch=<b> ...: the lines in $FAKE_RUNS/<b>, as
            // the real call's --jq would print them.
            fake(
                "gh",
                r#"[ -n "${FAKE_GH_FAIL:-}" ] && { echo "gh: HTTP 500" >&2; exit 1; }
for arg; do case $arg in branch=*) b=${arg#branch=};; esac; done
cat "$FAKE_RUNS/$b" 2>/dev/null || true"#,
            );
            // cargo audit --json --file <lock> ...: the lock is the report.
            // Each call's arguments are logged, one line per call.
            fake(
                "cargo",
                r#"echo "$*" >> "$FAKE_CARGO_LOG"
[ -n "${FAKE_CARGO_GARBAGE:-}" ] && { echo "error: couldn't fetch advisory database"; exit 1; }
while [ $# -gt 0 ]; do [ "$1" = --file ] && { cat "$2"; exit 1; }; shift; done
exit 2"#,
            );
            let scratch_path = scratch.path().to_path_buf();
            Fixture {
                _scratch: scratch,
                bin,
                scratch: scratch_path,
            }
        }

        fn path(&self) -> String {
            format!("{}:{}", self.bin.display(), std::env::var("PATH").unwrap())
        }

        fn repo(&self, name: &str) -> Repo {
            let dir = self.scratch.join(name);
            std::fs::create_dir(&dir).unwrap();
            let repo = Repo { dir };
            repo.git(&["init", "-q", "-b", "main"]);
            repo.git(&["config", "user.email", "test@example.com"]);
            repo.git(&["config", "user.name", "test"]);
            repo.git(&["config", "commit.gpgsign", "false"]);
            repo
        }

        /// Runs audit-new-advisories.sh; `AUDIT_BASE` is left unset unless
        /// `env` sets it.
        fn audit(&self, repo: &Repo, env: &[(&str, &str)]) -> (bool, String, String) {
            let log = self.scratch.join(format!(
                "cargo-{}.log",
                repo.dir.file_name().unwrap().to_string_lossy()
            ));
            let _ = std::fs::remove_file(&log);
            let mut command = Command::new("bash");
            command
                .arg(root().join("scripts/audit-new-advisories.sh"))
                .current_dir(&repo.dir)
                .env("PATH", self.path())
                .env("FAKE_CARGO_LOG", &log)
                .env_remove("AUDIT_BASE");
            for (key, value) in env {
                command.env(key, value);
            }
            let output = command.output().unwrap();
            (
                output.status.success(),
                text(&output),
                std::fs::read_to_string(&log).unwrap_or_default(),
            )
        }

        /// Runs audit-baseline.sh as CI would. `runs` maps a branch to the
        /// head SHAs of its successful runs, newest first. Returns the exit
        /// status, the output, and the `sha=` it wrote, if any.
        fn baseline(
            &self,
            repo: &Repo,
            runs: &[(&str, &[&str])],
            env: &[(&str, &str)],
        ) -> (bool, String, Option<String>) {
            let runs_dir = tempfile::tempdir_in(&self.scratch).unwrap().keep();
            for (branch, shas) in runs {
                let lines: Vec<String> = shas
                    .iter()
                    .enumerate()
                    .map(|(i, sha)| {
                        format!(
                            "2026-01-{:02}T00:00:00Z {sha} https://example.test/run/{i}",
                            28 - i
                        )
                    })
                    .collect();
                std::fs::write(runs_dir.join(branch), lines.join("\n") + "\n").unwrap();
            }
            let output_file = runs_dir.join("github-output");
            std::fs::write(&output_file, "").unwrap();
            let mut vars: HashMap<&str, String> = [
                ("GITHUB_ACTIONS", "true"),
                ("GITHUB_REPOSITORY", "owner/repo"),
                ("GITHUB_EVENT_NAME", "push"),
                ("GITHUB_REF_NAME", "main"),
                ("GITHUB_REF_TYPE", "branch"),
                ("GH_TOKEN", "token"),
                ("AUDIT_WORKFLOWS", "ci.yml"),
                ("AUDIT_DEFAULT_BRANCH", "main"),
            ]
            .into_iter()
            .map(|(k, v)| (k, v.to_string()))
            .collect();
            for (key, value) in env {
                vars.insert(key, value.to_string());
            }
            let output = Command::new("bash")
                .arg(root().join("scripts/audit-baseline.sh"))
                .current_dir(&repo.dir)
                .env("PATH", self.path())
                .env("GITHUB_OUTPUT", &output_file)
                .env("FAKE_RUNS", &runs_dir)
                .envs(vars)
                .output()
                .unwrap();
            let written = std::fs::read_to_string(&output_file).unwrap();
            let sha = written
                .lines()
                .find_map(|l| l.strip_prefix("sha="))
                .map(str::to_string);
            (output.status.success(), text(&output), sha)
        }
    }

    struct Repo {
        dir: PathBuf,
    }

    impl Repo {
        fn git(&self, args: &[&str]) -> String {
            let output = Command::new("git")
                .args(args)
                .current_dir(&self.dir)
                .output()
                .unwrap();
            assert!(output.status.success(), "git {args:?}: {}", text(&output));
            String::from_utf8(output.stdout).unwrap().trim().to_string()
        }

        /// Commits `report` as Cargo.lock; returns the new commit.
        fn commit(&self, report: &str, message: &str) -> String {
            std::fs::write(self.dir.join("Cargo.lock"), report).unwrap();
            self.git(&["add", "-A"]);
            self.git(&["commit", "-q", "--allow-empty", "-m", message]);
            self.git(&["rev-parse", "HEAD"])
        }
    }

    fn text(output: &Output) -> String {
        format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    }

    /// A `cargo audit --json` report: vulnerabilities by advisory id, and
    /// warnings as (kind, crate, advisory id or none for a yank).
    fn report(vulnerabilities: &[&str], warnings: &[(&str, &str, Option<&str>)]) -> String {
        let list: Vec<String> = vulnerabilities
            .iter()
            .map(|id| format!(r#"{{"advisory":{{"id":"{id}","title":"advisory {id}"}},"package":{{"name":"pkg","version":"1.0.0"}}}}"#))
            .collect();
        let mut kinds: Vec<String> = Vec::new();
        for kind in ["unmaintained", "unsound", "yanked"] {
            let entries: Vec<String> = warnings
                .iter()
                .filter(|(k, _, _)| *k == kind)
                .map(|(k, name, advisory)| {
                    let advisory = advisory.map_or("null".to_string(), |id| format!(r#"{{"id":"{id}","title":"advisory {id}"}}"#));
                    let (name, version) = name.split_once('@').unwrap_or((name, "1.0.0"));
                    format!(r#"{{"kind":"{k}","advisory":{advisory},"package":{{"name":"{name}","version":"{version}"}}}}"#)
                })
                .collect();
            if !entries.is_empty() {
                kinds.push(format!(r#""{kind}":[{}]"#, entries.join(",")));
            }
        }
        format!(
            r#"{{"vulnerabilities":{{"found":{},"count":{},"list":[{}]}},"warnings":{{{}}}}}"#,
            !list.is_empty(),
            list.len(),
            list.join(","),
            kinds.join(",")
        )
    }

    fn clean() -> String {
        report(&[], &[])
    }

    // ---- audit-new-advisories.sh ----

    #[test]
    fn fails_on_a_vulnerability_the_change_adds() {
        let f = Fixture::new();
        let repo = f.repo("r");
        repo.commit(&clean(), "base");
        repo.commit(&report(&["RUSTSEC-2026-0001"], &[]), "adds one");
        let (ok, out, _) = f.audit(&repo, &[]);
        assert!(!ok, "{out}");
        assert!(
            out.contains("::error title=New advisory::vulnerability RUSTSEC-2026-0001"),
            "{out}"
        );
    }

    #[test]
    fn only_warns_on_what_the_baseline_already_had() {
        let f = Fixture::new();
        let repo = f.repo("r");
        let had = report(
            &["RUSTSEC-2026-0001"],
            &[("unmaintained", "old", Some("RUSTSEC-2026-0002"))],
        );
        repo.commit(&had, "base");
        repo.commit(&had, "unrelated");
        let (ok, out, _) = f.audit(&repo, &[]);
        assert!(ok, "{out}");
        assert!(
            out.contains("::warning title=Existing advisory::vulnerability RUSTSEC-2026-0001"),
            "{out}"
        );
        assert!(
            out.contains("::warning title=Existing advisory::unmaintained RUSTSEC-2026-0002"),
            "{out}"
        );
    }

    #[test]
    fn fails_on_a_warning_the_change_adds_as_deny_warnings_did() {
        let f = Fixture::new();
        let repo = f.repo("r");
        repo.commit(&clean(), "base");
        repo.commit(
            &report(&[], &[("unsound", "pkg", Some("RUSTSEC-2026-0003"))]),
            "adds one",
        );
        let (ok, out, _) = f.audit(&repo, &[]);
        assert!(!ok, "{out}");
        assert!(
            out.contains("::error title=New advisory::unsound RUSTSEC-2026-0003"),
            "{out}"
        );
    }

    #[test]
    fn keys_a_yank_by_crate_and_version() {
        let f = Fixture::new();
        let repo = f.repo("r");
        repo.commit(&report(&[], &[("yanked", "pkg@1.0.0", None)]), "base");
        // The same crate yanked at another version is a new finding.
        repo.commit(
            &report(&[], &[("yanked", "pkg@1.0.1", None)]),
            "moves to another yanked version",
        );
        let (ok, out, _) = f.audit(&repo, &[]);
        assert!(!ok, "{out}");
        assert!(
            out.contains("::error title=New advisory::yanked pkg 1.0.1"),
            "{out}"
        );
    }

    #[test]
    fn audits_the_baseline_with_the_database_just_fetched() {
        let f = Fixture::new();
        let repo = f.repo("r");
        repo.commit(&clean(), "base");
        repo.commit(&clean(), "tip");
        let (ok, out, log) = f.audit(&repo, &[]);
        assert!(ok, "{out}");
        let calls: Vec<&str> = log.lines().collect();
        assert_eq!(calls.len(), 2, "{log}");
        assert!(!calls[0].contains("--no-fetch"), "{log}");
        assert!(calls[1].contains("--no-fetch"), "{log}");
    }

    #[test]
    fn compares_against_audit_base_not_the_parent_when_it_is_set() {
        let f = Fixture::new();
        let repo = f.repo("r");
        let green = repo.commit(&clean(), "green");
        repo.commit(&report(&["RUSTSEC-2026-0001"], &[]), "adds one");
        repo.commit(&report(&["RUSTSEC-2026-0001"], &[]), "unrelated tip");
        // Against the parent this would only warn: the multi-commit push.
        assert!(f.audit(&repo, &[]).0);
        assert!(!f.audit(&repo, &[("AUDIT_BASE", &green)]).0);
    }

    #[test]
    fn counts_everything_as_new_with_an_empty_audit_base() {
        let f = Fixture::new();
        let repo = f.repo("r");
        repo.commit(&report(&["RUSTSEC-2026-0001"], &[]), "base");
        repo.commit(&report(&["RUSTSEC-2026-0001"], &[]), "tip");
        let (ok, out, _) = f.audit(&repo, &[("AUDIT_BASE", "")]);
        assert!(!ok, "{out}");
        assert!(out.contains("every finding counts as new"), "{out}");
    }

    #[test]
    fn fails_closed_on_a_baseline_it_cannot_find() {
        let f = Fixture::new();
        let repo = f.repo("r");
        repo.commit(&clean(), "base");
        repo.commit(&clean(), "tip");
        let (ok, out, _) = f.audit(&repo, &[("AUDIT_BASE", "deadbeef")]);
        assert!(!ok, "{out}");
        assert!(out.contains("::error title=Audit failed::"), "{out}");
    }

    #[test]
    fn fails_closed_when_cargo_audit_produces_no_report() {
        let f = Fixture::new();
        let repo = f.repo("r");
        repo.commit(&clean(), "base");
        repo.commit(&clean(), "tip");
        let (ok, out, _) = f.audit(&repo, &[("FAKE_CARGO_GARBAGE", "1")]);
        assert!(!ok, "{out}");
        assert!(out.contains("produced no report"), "{out}");
    }

    // ---- audit-baseline.sh ----

    #[test]
    fn prints_the_first_parent_outside_actions() {
        let f = Fixture::new();
        let repo = f.repo("r");
        let first = repo.commit(&clean(), "one");
        repo.commit(&clean(), "two");
        let output = Command::new("bash")
            .arg(root().join("scripts/audit-baseline.sh"))
            .current_dir(&repo.dir)
            .env("GITHUB_ACTIONS", "")
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", text(&output));
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), first);
    }

    #[test]
    fn on_a_push_uses_the_last_green_run_however_many_commits_came_since() {
        let f = Fixture::new();
        let repo = f.repo("r");
        let green = repo.commit(&clean(), "green");
        repo.commit(&clean(), "unverified 1");
        repo.commit(&clean(), "unverified 2");
        let (ok, out, sha) = f.baseline(&repo, &[("main", &[&green])], &[]);
        assert!(ok, "{out}");
        assert_eq!(sha.as_deref(), Some(green.as_str()));
    }

    #[test]
    fn on_a_push_never_compares_a_commit_with_itself() {
        let f = Fixture::new();
        let repo = f.repo("r");
        let earlier = repo.commit(&clean(), "earlier green");
        let head = repo.commit(&clean(), "re-run of a green head");
        let (_, out, sha) = f.baseline(&repo, &[("main", &[&head, &earlier])], &[]);
        assert_eq!(sha.as_deref(), Some(earlier.as_str()), "{out}");
    }

    #[test]
    fn skips_runs_that_are_not_ancestors_or_no_longer_exist() {
        let f = Fixture::new();
        let repo = f.repo("r");
        let green = repo.commit(&clean(), "green");
        repo.git(&["checkout", "-q", "-b", "elsewhere"]);
        let elsewhere = repo.commit(&clean(), "not an ancestor of main");
        repo.git(&["checkout", "-q", "main"]);
        repo.commit(&clean(), "tip");
        let gone = "f".repeat(40);
        let (_, out, sha) = f.baseline(&repo, &[("main", &[&gone, &elsewhere, &green])], &[]);
        assert_eq!(sha.as_deref(), Some(green.as_str()), "{out}");
    }

    #[test]
    fn on_a_pull_request_uses_the_target_branchs_last_green_run_not_its_tip() {
        let f = Fixture::new();
        let repo = f.repo("r");
        let green = repo.commit(&clean(), "green main");
        repo.commit(
            &report(&["RUSTSEC-2026-0001"], &[]),
            "red direct push to main",
        );
        repo.git(&["checkout", "-q", "-b", "pr"]);
        repo.commit(&report(&["RUSTSEC-2026-0001"], &[]), "pr change");
        repo.git(&["checkout", "-q", "main"]);
        repo.git(&["merge", "-q", "--no-ff", "-m", "merge ref", "pr"]);
        let pr = [
            ("GITHUB_EVENT_NAME", "pull_request"),
            ("GITHUB_BASE_REF", "main"),
            ("GITHUB_REF_NAME", "1/merge"),
        ];
        let (_, out, sha) = f.baseline(&repo, &[("main", &[&green])], &pr);
        assert_eq!(sha.as_deref(), Some(green.as_str()), "{out}");
        // And the audit then fails the PR on what the red push brought in.
        assert!(!f.audit(&repo, &[("AUDIT_BASE", &green)]).0);
    }

    #[test]
    fn falls_back_to_the_default_branch_then_to_no_baseline_at_all() {
        let f = Fixture::new();
        let repo = f.repo("r");
        let green = repo.commit(&clean(), "green main");
        repo.git(&["checkout", "-q", "-b", "feature"]);
        repo.commit(&clean(), "feature work");
        let feature = [("GITHUB_REF_NAME", "feature")];
        let (_, out, sha) = f.baseline(&repo, &[("main", &[&green])], &feature);
        assert_eq!(sha.as_deref(), Some(green.as_str()), "{out}");
        let (ok, out, sha) = f.baseline(&repo, &[], &feature);
        assert!(ok, "{out}");
        assert_eq!(sha.as_deref(), Some(""));
        assert!(
            out.contains("every high or critical advisory counts as new"),
            "{out}"
        );
    }

    #[test]
    fn fails_when_the_run_lookup_fails_rather_than_guessing() {
        let f = Fixture::new();
        let repo = f.repo("r");
        let green = repo.commit(&clean(), "green");
        repo.commit(&clean(), "tip");
        let (ok, out, sha) = f.baseline(&repo, &[("main", &[&green])], &[("FAKE_GH_FAIL", "1")]);
        assert!(!ok, "{out}");
        assert_eq!(sha, None);
    }

    #[test]
    fn fails_in_a_shallow_clone() {
        let f = Fixture::new();
        let repo = f.repo("r");
        repo.commit(&clean(), "one");
        repo.commit(&clean(), "two");
        let shallow = f.scratch.join("shallow");
        let status = Command::new("git")
            .args(["clone", "-q", "--depth", "1"])
            .arg(format!("file://{}", repo.dir.display()))
            .arg(&shallow)
            .status()
            .unwrap();
        assert!(status.success());
        let (ok, out, _) = f.baseline(&Repo { dir: shallow }, &[], &[]);
        assert!(!ok, "{out}");
    }
}

// ---- ci.yml wiring ----

/// The `audit` job's block of ci.yml: from its key to the next job's.
fn audit_job() -> String {
    let ci = read(".github/workflows/ci.yml");
    let mut lines = ci.lines().skip_while(|line| *line != "  audit:");
    let mut job = vec![lines.next().expect("ci.yml has no `audit` job")];
    job.extend(lines.take_while(|line| {
        let indent = line.len() - line.trim_start().len();
        line.trim().is_empty() || line.trim_start().starts_with('#') || indent > 2
    }));
    job.join("\n")
}

/// The steps of a job block, each as its text from its `- ` to the next.
fn steps(job: &str) -> Vec<String> {
    let mut steps: Vec<String> = Vec::new();
    for line in job.lines() {
        if line.starts_with("      - ") {
            steps.push(String::new());
        }
        if let Some(step) = steps.last_mut()
            && !line.trim_start().starts_with('#')
        {
            step.push_str(line.trim());
            step.push('\n');
        }
    }
    steps
}

#[test]
fn the_audit_job_checks_out_the_full_history() {
    let job = audit_job();
    let checkout = steps(&job)
        .into_iter()
        .find(|s| s.contains("uses: actions/checkout@"))
        .expect("no checkout");
    assert!(checkout.contains("\nfetch-depth: 0\n"), "{checkout}");
}

#[test]
fn the_audit_job_may_look_up_earlier_runs() {
    let job = audit_job();
    let permissions: Vec<&str> = job
        .lines()
        .skip_while(|l| *l != "    permissions:")
        .skip(1)
        .take_while(|l| l.starts_with("      ") && !l.starts_with("      - "))
        .map(str::trim)
        .filter(|l| !l.starts_with('#'))
        .collect();
    assert!(permissions.contains(&"actions: read"), "{permissions:?}");
}

#[test]
fn the_audit_compares_against_the_baseline_ci_found() {
    let steps = steps(&audit_job());
    let baseline = steps
        .iter()
        .position(|s| s.contains("\nid: audit-baseline\n"))
        .expect("no audit-baseline step");
    let audit = steps
        .iter()
        .position(|s| s.contains("\nrun: scripts/audit-new-advisories.sh\n"))
        .expect("no audit step");
    assert!(
        baseline < audit,
        "the baseline must be found before the audit runs"
    );

    let step = &steps[baseline];
    assert!(
        step.contains("\nrun: scripts/audit-baseline.sh\n"),
        "{step}"
    );
    assert!(step.contains("\nGH_TOKEN: ${{ github.token }}\n"), "{step}");
    assert!(
        step.contains("\nAUDIT_DEFAULT_BRANCH: ${{ github.event.repository.default_branch }}\n"),
        "{step}"
    );
    let workflows = step
        .lines()
        .find_map(|l| l.strip_prefix("AUDIT_WORKFLOWS: "))
        .expect("no AUDIT_WORKFLOWS");
    for workflow in workflows.split(',') {
        assert!(
            root().join(".github/workflows").join(workflow).is_file(),
            "{workflow} does not exist"
        );
    }

    assert!(
        steps[audit].contains("\nAUDIT_BASE: ${{ steps.audit-baseline.outputs.sha }}\n"),
        "the audit must be handed the baseline: {}",
        steps[audit]
    );
}
