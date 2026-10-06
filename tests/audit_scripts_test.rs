//! The dependency audit fails a change only for the RustSec findings it adds
//! over a baseline commit, so what decides the baseline
//! (scripts/audit-baseline.sh) and what counts as added
//! (scripts/audit-new-advisories.sh) are the whole of the gate. An earlier
//! version compared a push only with its parent, and a pull request with the
//! target branch's tip; both let a finding through, and both are pinned here.
//!
//! The scripts are driven end to end against throwaway repositories, with
//! fakes first on PATH. `gh` serves a list of workflow runs the way the API
//! does -- filtered by workflow, branch and `status` -- through the script's
//! own `--jq` program with the real jq, so the filters that keep a failed
//! run or a pull request's run from becoming a baseline are exercised rather
//! than assumed. `cargo audit --json --file <lock>` prints the lock itself --
//! each commit's Cargo.lock here *is* the report its audit would produce, so
//! the baseline's copy reports its own. Every child gets an environment with
//! no GITHUB_*, GIT_* or AUDIT_* variables and no global git config, so the
//! CI runner these tests run on cannot change what they find. They are bash
//! scripts that only ever run on Linux CI, so these cases are Unix-only; the
//! wiring checks at the bottom run everywhere.
//!
//! Separately, ci.yml and ci-gate.yml have to give the baseline step the full
//! history, `actions: read`, and workflows that run the audit, hand the audit
//! what it found, and never let `gate` skip it. None of those fails visibly
//! when wrong -- a dropped AUDIT_BASE quietly falls back to comparing against
//! HEAD's parent -- so they are pinned too.

use std::path::Path;

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
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::process::{Command, Output};
    use tempfile::TempDir;

    struct Fixture {
        _scratch: TempDir,
        bin: PathBuf,
        scratch: PathBuf,
    }

    /// A workflow run as the API lists it. Newest first unless `at` says when.
    #[derive(Default)]
    struct Run<'a> {
        sha: &'a str,
        branch: Option<&'a str>,
        workflow: Option<&'a str>,
        event: Option<&'a str>,
        conclusion: Option<&'a str>,
        at: Option<&'a str>,
    }

    fn run(sha: &str) -> Run<'_> {
        Run {
            sha,
            ..Run::default()
        }
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
            // gh api -X GET repos/<repo>/actions/workflows/<file>/runs
            // -f branch=<b> [-f status=<s>] ... --jq <program>: the runs in
            // $FAKE_RUNS/runs.json for that workflow and branch -- and
            // conclusion, if asked -- as {workflow_runs: [...]}, through
            // <program>.
            fake(
                "gh",
                r#"[ -n "${FAKE_GH_FAIL:-}" ] && { echo "gh: HTTP 500" >&2; exit 1; }
workflow="" branch="" status="" program=""
while [ $# -gt 0 ]; do
  case $1 in
    */actions/workflows/*/runs) workflow=${1%/runs}; workflow=${workflow##*/} ;;
    -f) case $2 in branch=*) branch=${2#branch=} ;; status=*) status=${2#status=} ;; esac; shift ;;
    --jq) program=$2; shift ;;
  esac
  shift
done
jq --arg w "$workflow" --arg b "$branch" --arg s "$status" \
  '{workflow_runs: [.[] | select(.workflow == $w and .head_branch == $b and ($s == "" or .conclusion == $s))]}' \
  "$FAKE_RUNS/runs.json" | jq -r "$program""#,
            );
            // cargo audit --json --file <lock> ...: the lock is the report.
            // Each call's arguments are logged, one line per call;
            // FAKE_CARGO_STDERR is printed to stderr, as cargo audit reports
            // what it could not check -- for every lock, or only for one
            // whose path ends in FAKE_CARGO_STDERR_FOR. FAKE_CANARY sets
            // what the canary reports yanked: libc 0.2.165 by default,
            // `none`, or another crate.
            fake(
                "cargo",
                r#"echo "$*" >> "$FAKE_CARGO_LOG"
[ -n "${FAKE_CARGO_GARBAGE:-}" ] && { echo "error: couldn't fetch advisory database"; exit 1; }
file=""; prev=""; for a; do [ "$prev" = --file ] && file=$a; prev=$a; done
if [ -n "${FAKE_CARGO_STDERR:-}" ]; then
  case $file in *"${FAKE_CARGO_STDERR_FOR:-}") echo "$FAKE_CARGO_STDERR" >&2 ;; esac
fi
while [ $# -gt 0 ]; do
  if [ "$1" = --file ]; then
    # The canary: a yank check that works reports libc 0.2.165.
    case $2 in */canary.lock)
      c=${FAKE_CANARY:-libc@0.2.165}
      case $c in
        none) yanked='' ;;
        *) yanked="{\"kind\":\"yanked\",\"advisory\":null,\"package\":{\"name\":\"${c%@*}\",\"version\":\"${c#*@}\"}}" ;;
      esac
      echo "{\"vulnerabilities\":{\"found\":false,\"count\":0,\"list\":[]},\"warnings\":{\"yanked\":[$yanked]}}"
      exit 1 ;;
    esac
    cat "$2"; exit 1
  fi
  shift
done
exit 2"#,
            );
            let scratch_path = scratch.path().to_path_buf();
            Fixture {
                _scratch: scratch,
                bin,
                scratch: scratch_path,
            }
        }

        /// A command with a scrubbed environment and the fakes on PATH.
        fn command(&self, program: &str) -> Command {
            let mut command = Command::new(program);
            for (key, _) in std::env::vars_os() {
                let key = key.to_string_lossy();
                if ["GITHUB_", "GIT_", "AUDIT_"]
                    .iter()
                    .any(|p| key.starts_with(p))
                {
                    command.env_remove(&*key);
                }
            }
            command
                .env(
                    "PATH",
                    format!("{}:{}", self.bin.display(), std::env::var("PATH").unwrap()),
                )
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1");
            command
        }

        fn repo(&self, name: &str) -> Repo<'_> {
            let dir = self.scratch.join(name);
            std::fs::create_dir(&dir).unwrap();
            let repo = Repo { dir, fixture: self };
            repo.git(&["init", "-q", "-b", "main"]);
            repo.git(&["config", "user.email", "test@example.com"]);
            repo.git(&["config", "user.name", "test"]);
            repo.git(&["config", "commit.gpgsign", "false"]);
            repo
        }

        /// Runs audit-new-advisories.sh; `AUDIT_BASE` is unset unless `env`
        /// sets it. Returns success, the output, and cargo's logged calls.
        fn audit(&self, repo: &Repo, env: &[(&str, &str)]) -> (bool, String, String) {
            self.audit_with(repo, env, &[])
        }

        /// Runs audit-new-advisories.sh with arguments.
        fn audit_with(
            &self,
            repo: &Repo,
            env: &[(&str, &str)],
            args: &[&str],
        ) -> (bool, String, String) {
            let log = self.scratch.join(format!(
                "cargo-{}.log",
                repo.dir.file_name().unwrap().to_string_lossy()
            ));
            let _ = std::fs::remove_file(&log);
            let output = self
                .command("bash")
                .arg(root().join("scripts/audit-new-advisories.sh"))
                .args(args)
                .current_dir(&repo.dir)
                .env("FAKE_CARGO_LOG", &log)
                .envs(env.iter().copied())
                .output()
                .unwrap();
            (
                output.status.success(),
                text(&output),
                std::fs::read_to_string(&log).unwrap_or_default(),
            )
        }

        /// Runs audit-baseline.sh as CI would. Returns the exit status, the
        /// output, and the `sha=` it wrote, if any.
        fn baseline(
            &self,
            repo: &Repo,
            runs: &[Run],
            env: &[(&str, &str)],
        ) -> (bool, String, Option<String>) {
            let runs_dir = tempfile::tempdir_in(&self.scratch).unwrap().keep();
            let api: Vec<String> = runs
                .iter()
                .enumerate()
                .map(|(i, run)| {
                    let at = run
                        .at
                        .map_or_else(|| format!("2026-01-{:02}T00:00:00Z", 28 - i), str::to_string);
                    format!(
                        r#"{{"workflow":"{}","head_branch":"{}","head_sha":"{}","event":"{}","status":"completed","conclusion":"{}","created_at":"{at}","html_url":"https://example.test/run/{i}"}}"#,
                        run.workflow.unwrap_or("ci.yml"),
                        run.branch.unwrap_or("main"),
                        run.sha,
                        run.event.unwrap_or("push"),
                        run.conclusion.unwrap_or("success"),
                    )
                })
                .collect();
            std::fs::write(runs_dir.join("runs.json"), format!("[{}]", api.join(","))).unwrap();
            let output_file = runs_dir.join("github-output");
            std::fs::write(&output_file, "").unwrap();
            let output = self
                .command("bash")
                .arg(root().join("scripts/audit-baseline.sh"))
                .current_dir(&repo.dir)
                .env("GITHUB_OUTPUT", &output_file)
                .env("FAKE_RUNS", &runs_dir)
                .envs([
                    ("GITHUB_ACTIONS", "true"),
                    ("GITHUB_REPOSITORY", "owner/repo"),
                    ("GITHUB_EVENT_NAME", "push"),
                    ("GITHUB_REF_NAME", "main"),
                    ("GITHUB_REF_TYPE", "branch"),
                    ("GH_TOKEN", "token"),
                    ("AUDIT_WORKFLOWS", "ci.yml"),
                    ("AUDIT_DEFAULT_BRANCH", "main"),
                ])
                .envs(env.iter().copied())
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

    struct Repo<'a> {
        dir: PathBuf,
        fixture: &'a Fixture,
    }

    impl Repo<'_> {
        fn git(&self, args: &[&str]) -> String {
            let output = self
                .fixture
                .command("git")
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

    /// A `cargo audit --json` report. Each vulnerability is an advisory id,
    /// optionally followed by the crate it is in -- `"RUSTSEC-1 pkg@1.0.0"`,
    /// pkg 1.0.0 when left out. Warnings are (kind, crate[@version], advisory
    /// id or none for a yank).
    fn report(vulnerabilities: &[&str], warnings: &[(&str, &str, Option<&str>)]) -> String {
        let list: Vec<String> = vulnerabilities
            .iter()
            .map(|v| {
                let (id, krate) = v.split_once(' ').unwrap_or((v, "pkg@1.0.0"));
                let (name, version) = krate.split_once('@').unwrap();
                format!(
                    r#"{{"advisory":{{"id":"{id}","title":"advisory {id}"}},"package":{{"name":"{name}","version":"{version}"}}}}"#
                )
            })
            .collect();
        let mut kinds: Vec<String> = Vec::new();
        for kind in ["unmaintained", "unsound", "yanked"] {
            let entries: Vec<String> = warnings
                .iter()
                .filter(|(k, _, _)| *k == kind)
                .map(|(k, name, advisory)| {
                    let advisory = advisory.map_or("null".to_string(), |id| {
                        format!(r#"{{"id":"{id}","title":"advisory {id}"}}"#)
                    });
                    let (name, version) = name.split_once('@').unwrap_or((name, "1.0.0"));
                    format!(
                        r#"{{"kind":"{k}","advisory":{advisory},"package":{{"name":"{name}","version":"{version}"}}}}"#
                    )
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

    #[track_caller]
    fn assert_has(out: &str, text: &str) {
        assert!(out.contains(text), "expected {text:?} in:\n{out}");
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
        assert_has(
            &out,
            "::error title=New advisory::vulnerability RUSTSEC-2026-0001",
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
        assert_has(
            &out,
            "::warning title=Existing advisory::vulnerability RUSTSEC-2026-0001",
        );
        assert_has(
            &out,
            "::warning title=Existing advisory::unmaintained RUSTSEC-2026-0002",
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
        assert_has(
            &out,
            "::error title=New advisory::unsound RUSTSEC-2026-0003",
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
        assert_has(&out, "::error title=New advisory::yanked pkg 1.0.1");
    }

    #[test]
    fn counts_a_copy_older_than_any_the_baseline_had_as_new() {
        let f = Fixture::new();
        let repo = f.repo("r");
        repo.commit(&report(&["RUSTSEC-2021-0003 smallvec@1.6.0"], &[]), "base");
        repo.commit(
            &report(
                &[
                    "RUSTSEC-2021-0003 smallvec@1.6.0",
                    "RUSTSEC-2021-0003 smallvec@0.6.13",
                ],
                &[],
            ),
            "an older vulnerable copy beside the first",
        );
        let (ok, out, _) = f.audit(&repo, &[]);
        assert!(!ok, "{out}");
        assert_has(
            &out,
            "smallvec 0.6.13: advisory RUSTSEC-2021-0003 -- already on the baseline, but this copy is older than any the baseline had (1.6.0)",
        );
    }

    #[test]
    fn counts_an_advisory_as_new_once_it_reaches_another_crate() {
        let f = Fixture::new();
        let repo = f.repo("r");
        repo.commit(&report(&["RUSTSEC-2026-0009 one@1.0.0"], &[]), "base");
        repo.commit(
            &report(
                &["RUSTSEC-2026-0009 one@1.0.0", "RUSTSEC-2026-0009 two@1.0.0"],
                &[],
            ),
            "reaches a second crate",
        );
        let (ok, out, _) = f.audit(&repo, &[]);
        assert!(!ok, "{out}");
        assert_has(&out, "it now reaches two, which it did not there");
    }

    #[test]
    fn does_not_count_a_partial_fix_which_leaves_the_old_copy_for_the_rest_as_new() {
        // One dependent moving to a newer, still-affected version while
        // another stays: in either version line, it is not a downgrade.
        let f = Fixture::new();
        let repo = f.repo("r");
        let had = ["RUSTSEC-1 pkg@2.1.4", "RUSTSEC-1 pkg@5.0.9"];
        repo.commit(&report(&had, &[]), "base");
        repo.commit(
            &report(&[had[0], had[1], "RUSTSEC-1 pkg@5.0.10"], &[]),
            "some move to 5.0.10",
        );
        let (ok, out, _) = f.audit(&repo, &[]);
        assert!(ok, "{out}");
        repo.commit(
            &report(&[had[0], "RUSTSEC-1 pkg@2.1.5", had[1]], &[]),
            "some move to 2.1.5",
        );
        let (ok, out, _) = f.audit(&repo, &[]);
        assert!(ok, "{out}");
    }

    #[test]
    fn does_not_count_moving_the_one_copy_to_another_affected_version_as_new() {
        // The update a fix arrives through: failing it would block the fix.
        let f = Fixture::new();
        let repo = f.repo("r");
        repo.commit(&report(&["RUSTSEC-2021-0003 smallvec@1.6.0"], &[]), "base");
        repo.commit(
            &report(&["RUSTSEC-2021-0003 smallvec@1.6.1"], &[]),
            "patch bump, still affected",
        );
        let (ok, out, _) = f.audit(&repo, &[]);
        assert!(ok, "{out}");
        assert_has(
            &out,
            "::warning title=Existing advisory::vulnerability RUSTSEC-2021-0003",
        );
    }

    #[test]
    fn fails_when_the_yank_check_is_lost_without_a_word() {
        // An index cargo audit cannot open is reported nowhere under --json:
        // only the canary going unreported shows it.
        let f = Fixture::new();
        let repo = f.repo("r");
        repo.commit(&clean(), "base");
        repo.commit(&clean(), "tip");
        let (ok, out, _) = f.audit(&repo, &[("FAKE_CANARY", "none")]);
        assert!(!ok, "{out}");
        assert_has(
            &out,
            "::error title=Audit failed::cargo audit did not report libc 0.2.165 as yanked",
        );
    }

    #[test]
    fn fails_unless_the_canary_reports_that_very_yank() {
        // Some other crate reported yanked proves nothing about this one.
        let f = Fixture::new();
        let repo = f.repo("r");
        repo.commit(&clean(), "base");
        repo.commit(&clean(), "tip");
        let (ok, out, _) = f.audit(&repo, &[("FAKE_CANARY", "other@1.0.0")]);
        assert!(!ok, "{out}");
        assert_has(&out, "did not report libc 0.2.165 as yanked");
        assert_has(&out, "--no-yanked, or [yanked] enabled = false");
    }

    #[test]
    fn passes_its_arguments_to_every_run() {
        let f = Fixture::new();
        let repo = f.repo("r");
        repo.commit(&clean(), "base");
        repo.commit(&clean(), "tip");
        let (ok, out, log) = f.audit_with(&repo, &[], &["--ignore", "RUSTSEC-0000-0000"]);
        assert!(ok, "{out}");
        assert_eq!(log.lines().count(), 3, "{log}");
        for call in log.lines() {
            assert!(call.ends_with("--ignore RUSTSEC-0000-0000"), "{log}");
        }
    }

    #[test]
    fn only_warns_when_the_baseline_has_a_crate_it_cannot_look_up() {
        // A crate crates.io has since deleted: failing on it would fail the
        // change that removes it, and every one after.
        let f = Fixture::new();
        let repo = f.repo("r");
        repo.commit(&clean(), "base");
        repo.commit(&clean(), "tip");
        let (ok, out, _) = f.audit(
            &repo,
            &[
                (
                    "FAKE_CARGO_STDERR",
                    "error: couldn't check if the package is yanked: not found: No such crate in crates.io index: gone",
                ),
                ("FAKE_CARGO_STDERR_FOR", "/base.lock"),
            ],
        );
        assert!(ok, "{out}");
        assert_has(
            &out,
            "::warning title=Audit baseline::cargo audit could not check every crate in the baseline's lock",
        );
    }

    #[test]
    fn orders_versions_by_semver() {
        // [baseline copy, new copy, counts as new]
        let cases = [
            ("1.10.0", "1.9.0", true),
            ("1.9.0", "1.10.0", false),
            ("1.0.0", "1.0.0-rc.1", true),
            ("1.0.0-rc.2", "1.0.0-rc.10", false),
            ("1.0.0-alpha.10", "1.0.0-alpha.2", true),
            ("1.0.0-alpha.1", "1.0.0-alpha", true),
            ("1.0.0-alpha", "1.0.0-1", true),
            ("1.0.0", "1.0.1+build.5", false),
            ("1.0.0+build.9", "0.9.0", true),
            ("1.0.0", "latest", true),
        ];
        for (was, now, is_new) in cases {
            let f = Fixture::new();
            let repo = f.repo("r");
            let base = format!("RUSTSEC-1 pkg@{was}");
            let both = [base.as_str(), &format!("RUSTSEC-1 pkg@{now}")];
            repo.commit(&report(&[&base], &[]), "base");
            repo.commit(&report(&both, &[]), "a second copy");
            let (ok, out, _) = f.audit(&repo, &[]);
            assert_eq!(!ok, is_new, "baseline {was}, new copy {now}: {out}");
        }
    }

    #[test]
    fn fails_when_cargo_audit_could_not_check_for_yanks() {
        let f = Fixture::new();
        let repo = f.repo("r");
        repo.commit(&clean(), "base");
        repo.commit(&clean(), "tip");
        let (ok, out, _) = f.audit(
            &repo,
            &[(
                "FAKE_CARGO_STDERR",
                "error: couldn't check if the package is yanked: not found: No such crate in crates.io index: libc",
            )],
        );
        assert!(!ok, "{out}");
        assert_has(
            &out,
            "::error title=Audit failed::cargo audit could not check Cargo.lock for yanked crates",
        );
    }

    #[test]
    fn audits_both_locks_and_the_canary_with_a_fresh_database() {
        // --no-fetch would confine the baseline's yank check to the index
        // entries HEAD's audit cached, so every crate the change removed
        // would fail to look up.
        let f = Fixture::new();
        let repo = f.repo("r");
        repo.commit(&clean(), "base");
        repo.commit(&clean(), "tip");
        let (ok, out, log) = f.audit(&repo, &[]);
        assert!(ok, "{out}");
        let calls: Vec<&str> = log.lines().collect();
        assert_eq!(calls.len(), 3, "{log}");
        assert!(
            calls[0].starts_with("audit --json --file Cargo.lock"),
            "{log}"
        );
        assert!(calls[1].ends_with("/canary.lock"), "{log}");
        assert!(calls[2].ends_with("/base.lock"), "{log}");
        assert!(!log.contains("--no-fetch"), "{log}");
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
        let (ok, out, _) = f.audit(&repo, &[("AUDIT_BASE", &green)]);
        assert!(!ok, "{out}");
        assert_has(
            &out,
            "::error title=New advisory::vulnerability RUSTSEC-2026-0001",
        );
    }

    #[test]
    fn counts_everything_as_new_with_an_empty_audit_base() {
        let f = Fixture::new();
        let repo = f.repo("r");
        repo.commit(&report(&["RUSTSEC-2026-0001"], &[]), "base");
        repo.commit(&report(&["RUSTSEC-2026-0001"], &[]), "tip");
        let (ok, out, _) = f.audit(&repo, &[("AUDIT_BASE", "")]);
        assert!(!ok, "{out}");
        assert_has(&out, "every finding counts as new");
        assert_has(
            &out,
            "::error title=New advisory::vulnerability RUSTSEC-2026-0001",
        );
    }

    #[test]
    fn refuses_to_fall_back_to_the_parent_in_ci() {
        let f = Fixture::new();
        let repo = f.repo("r");
        repo.commit(&clean(), "base");
        repo.commit(&clean(), "tip");
        let (ok, out, _) = f.audit(&repo, &[("GITHUB_ACTIONS", "true")]);
        assert!(!ok, "{out}");
        assert_has(&out, "::error title=Audit failed::AUDIT_BASE is not set");
    }

    #[test]
    fn fails_closed_on_a_baseline_it_cannot_find() {
        let f = Fixture::new();
        let repo = f.repo("r");
        repo.commit(&clean(), "base");
        repo.commit(&clean(), "tip");
        let (ok, out, _) = f.audit(&repo, &[("AUDIT_BASE", "deadbeef")]);
        assert!(!ok, "{out}");
        assert_has(
            &out,
            "::error title=Audit failed::the baseline commit deadbeef is not in this clone",
        );
    }

    #[test]
    fn fails_closed_when_cargo_audit_produces_no_report() {
        let f = Fixture::new();
        let repo = f.repo("r");
        repo.commit(&clean(), "base");
        repo.commit(&clean(), "tip");
        let (ok, out, _) = f.audit(&repo, &[("FAKE_CARGO_GARBAGE", "1")]);
        assert!(!ok, "{out}");
        assert_has(&out, "produced no report");
    }

    // ---- audit-baseline.sh ----

    const PULL_REQUEST: [(&str, &str); 3] = [
        ("GITHUB_EVENT_NAME", "pull_request"),
        ("GITHUB_BASE_REF", "main"),
        ("GITHUB_REF_NAME", "1/merge"),
    ];

    #[test]
    fn prints_the_first_parent_outside_actions() {
        let f = Fixture::new();
        let repo = f.repo("r");
        let first = repo.commit(&clean(), "one");
        repo.commit(&clean(), "two");
        let output = f
            .command("bash")
            .arg(root().join("scripts/audit-baseline.sh"))
            .current_dir(&repo.dir)
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
        let (ok, out, sha) = f.baseline(&repo, &[run(&green)], &[]);
        assert!(ok, "{out}");
        assert_eq!(sha.as_deref(), Some(green.as_str()));
    }

    #[test]
    fn skips_a_run_that_failed_however_recent() {
        let f = Fixture::new();
        let repo = f.repo("r");
        let green = repo.commit(&clean(), "green");
        let red = repo.commit(&report(&["RUSTSEC-2026-0001"], &[]), "red direct push");
        repo.commit(&report(&["RUSTSEC-2026-0001"], &[]), "tip");
        let runs = [
            Run {
                conclusion: Some("failure"),
                ..run(&red)
            },
            run(&green),
        ];
        let (_, out, sha) = f.baseline(&repo, &runs, &[]);
        assert_eq!(sha.as_deref(), Some(green.as_str()), "{out}");
    }

    #[test]
    fn skips_a_pull_requests_run_even_on_a_branch_of_the_same_name() {
        // A fork's branch can be called main; its run tested a merge commit,
        // not the head it reports.
        let f = Fixture::new();
        let repo = f.repo("r");
        let green = repo.commit(&clean(), "green");
        let pr = repo.commit(&clean(), "a pull request head, later merged");
        repo.commit(&clean(), "tip");
        let runs = [
            Run {
                event: Some("pull_request"),
                ..run(&pr)
            },
            run(&green),
        ];
        let (_, out, sha) = f.baseline(&repo, &runs, &[]);
        assert_eq!(sha.as_deref(), Some(green.as_str()), "{out}");
    }

    #[test]
    fn takes_the_newest_green_run_across_every_listed_workflow() {
        let f = Fixture::new();
        let repo = f.repo("r");
        let older = repo.commit(&clean(), "green in ci.yml");
        let newer = repo.commit(&clean(), "green in another workflow");
        repo.commit(&clean(), "tip");
        let runs = [
            Run {
                workflow: Some("ci.yml"),
                at: Some("2026-02-01T00:00:00Z"),
                ..run(&older)
            },
            Run {
                workflow: Some("other.yml"),
                at: Some("2026-02-02T00:00:00Z"),
                ..run(&newer)
            },
        ];
        let (_, out, sha) = f.baseline(&repo, &runs, &[("AUDIT_WORKFLOWS", "ci.yml,other.yml")]);
        assert_eq!(sha.as_deref(), Some(newer.as_str()), "{out}");
    }

    #[test]
    fn on_a_push_never_compares_a_commit_with_itself() {
        let f = Fixture::new();
        let repo = f.repo("r");
        let earlier = repo.commit(&clean(), "earlier green");
        let head = repo.commit(&clean(), "re-run of a green head");
        let (_, out, sha) = f.baseline(&repo, &[run(&head), run(&earlier)], &[]);
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
        let (_, out, sha) = f.baseline(&repo, &[run(&gone), run(&elsewhere), run(&green)], &[]);
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
        let (_, out, sha) = f.baseline(&repo, &[run(&green)], &PULL_REQUEST);
        assert_eq!(sha.as_deref(), Some(green.as_str()), "{out}");
        // And the audit then fails the PR on what the red push brought in.
        let (ok, out, _) = f.audit(&repo, &[("AUDIT_BASE", &green)]);
        assert!(!ok, "{out}");
        assert_has(
            &out,
            "::error title=New advisory::vulnerability RUSTSEC-2026-0001",
        );
    }

    #[test]
    fn on_a_pull_request_reads_ancestry_from_the_merges_first_parent() {
        // main was force-pushed back past a green commit the PR still
        // contains: that commit is in the merge, but no longer on the branch
        // it targets.
        let f = Fixture::new();
        let repo = f.repo("r");
        let kept = repo.commit(&clean(), "green, still on main");
        let dropped = repo.commit(&clean(), "green, later dropped from main");
        repo.git(&["checkout", "-q", "-b", "pr"]);
        repo.commit(&clean(), "pr change");
        repo.git(&["checkout", "-q", "main"]);
        repo.git(&["reset", "-q", "--hard", &kept]);
        repo.git(&["merge", "-q", "--no-ff", "-m", "merge ref", "pr"]);
        let (_, out, sha) = f.baseline(&repo, &[run(&dropped), run(&kept)], &PULL_REQUEST);
        assert_eq!(sha.as_deref(), Some(kept.as_str()), "{out}");
    }

    #[test]
    fn on_a_pull_request_into_another_branch_uses_that_branch() {
        let f = Fixture::new();
        let repo = f.repo("r");
        let on_main = repo.commit(&clean(), "green main");
        repo.git(&["checkout", "-q", "-b", "release"]);
        let on_release = repo.commit(&clean(), "green release");
        repo.git(&["checkout", "-q", "-b", "pr"]);
        repo.commit(&clean(), "pr change");
        repo.git(&["checkout", "-q", "release"]);
        repo.git(&["merge", "-q", "--no-ff", "-m", "merge ref", "pr"]);
        let runs = [
            Run {
                branch: Some("release"),
                ..run(&on_release)
            },
            run(&on_main),
        ];
        let mut env = PULL_REQUEST.to_vec();
        env.push(("GITHUB_BASE_REF", "release"));
        let (_, out, sha) = f.baseline(&repo, &runs, &env);
        assert_eq!(sha.as_deref(), Some(on_release.as_str()), "{out}");
    }

    #[test]
    fn for_a_tag_uses_the_default_branch_and_not_the_tagged_commit() {
        let f = Fixture::new();
        let repo = f.repo("r");
        let green = repo.commit(&clean(), "green main");
        let decoy = repo.commit(&clean(), "green on a branch named like the tag");
        let tagged = repo.commit(&clean(), "tagged, and green on main");
        let runs = [
            run(&tagged),
            Run {
                branch: Some("v1.0.0"),
                ..run(&decoy)
            },
            run(&green),
        ];
        let env = [("GITHUB_REF_TYPE", "tag"), ("GITHUB_REF_NAME", "v1.0.0")];
        let (_, out, sha) = f.baseline(&repo, &runs, &env);
        assert_eq!(sha.as_deref(), Some(green.as_str()), "{out}");
    }

    #[test]
    fn falls_back_to_the_default_branch_then_to_no_baseline_at_all() {
        let f = Fixture::new();
        let repo = f.repo("r");
        let green = repo.commit(&clean(), "green main");
        repo.git(&["checkout", "-q", "-b", "feature"]);
        repo.commit(&clean(), "feature work");
        let feature = [("GITHUB_REF_NAME", "feature")];
        let (_, out, sha) = f.baseline(&repo, &[run(&green)], &feature);
        assert_eq!(sha.as_deref(), Some(green.as_str()), "{out}");
        let (ok, out, sha) = f.baseline(&repo, &[], &feature);
        assert!(ok, "{out}");
        assert_eq!(sha.as_deref(), Some(""));
        assert_has(&out, "everything the audit finds counts as new");
    }

    #[test]
    fn fails_when_the_run_lookup_fails_rather_than_guessing() {
        let f = Fixture::new();
        let repo = f.repo("r");
        let green = repo.commit(&clean(), "green");
        repo.commit(&clean(), "tip");
        let (ok, out, sha) = f.baseline(&repo, &[run(&green)], &[("FAKE_GH_FAIL", "1")]);
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
        let status = f
            .command("git")
            .args(["clone", "-q", "--depth", "1"])
            .arg(format!("file://{}", repo.dir.display()))
            .arg(&shallow)
            .status()
            .unwrap();
        assert!(status.success());
        let clone = Repo {
            dir: shallow,
            fixture: &f,
        };
        let (ok, out, sha) = f.baseline(&clone, &[], &[]);
        assert!(!ok, "{out}");
        assert_has(&out, "shallow");
        assert_eq!(sha, None);
    }
}

// ---- workflow wiring ----

/// A job's block of a workflow: from its key to the next job's.
fn job(workflow: &str, name: &str) -> String {
    let text = read(&format!(".github/workflows/{workflow}"));
    let key = format!("  {name}:");
    let mut lines = text.lines().skip_while(|line| *line != key);
    let mut job = vec![
        lines
            .next()
            .unwrap_or_else(|| panic!("{workflow} has no `{name}` job")),
    ];
    job.extend(lines.take_while(|line| {
        let indent = line.len() - line.trim_start().len();
        line.trim().is_empty() || line.trim_start().starts_with('#') || indent > 2
    }));
    job.join("\n")
}

/// The job's own keys, before its steps: `    key: value` lines.
fn job_keys(job: &str) -> Vec<String> {
    job.lines()
        .skip(1)
        .take_while(|line| *line != "    steps:")
        .filter(|line| {
            line.starts_with("    ")
                && !line.starts_with("     ")
                && !line.trim_start().starts_with('#')
        })
        .map(|line| line.trim().to_string())
        .collect()
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
            // Without the `- `, so a step's first key reads like its others.
            step.push_str(line.trim().trim_start_matches("- "));
            step.push('\n');
        }
    }
    steps
}

/// Every audit job: ci.yml's, on pushes and pull requests, and ci-gate.yml's,
/// on releases.
const AUDIT_JOBS: [&str; 2] = ["ci.yml", "ci-gate.yml"];

#[test]
fn the_audit_job_never_skips_and_never_fails_quietly() {
    for workflow in AUDIT_JOBS {
        let job = job(workflow, "audit");
        for key in job_keys(&job) {
            for forbidden in ["if:", "needs:", "continue-on-error:"] {
                assert!(
                    !key.starts_with(forbidden),
                    "{workflow}'s audit job has `{key}`"
                );
            }
        }
        for step in steps(&job)
            .iter()
            .filter(|s| s.contains("\nrun: scripts/audit-"))
        {
            assert!(
                !step.contains("\nif:") && !step.contains("\ncontinue-on-error:"),
                "{workflow}: {step}"
            );
        }
    }
}

#[test]
fn the_audit_job_checks_out_the_full_history() {
    for workflow in AUDIT_JOBS {
        let job = job(workflow, "audit");
        let checkout = steps(&job)
            .into_iter()
            .find(|s| s.contains("uses: actions/checkout@"))
            .unwrap_or_else(|| panic!("{workflow}: no checkout"));
        assert!(
            checkout.contains("\nfetch-depth: 0\n"),
            "{workflow}: {checkout}"
        );
    }
}

#[test]
fn the_audit_job_may_look_up_earlier_runs() {
    for workflow in AUDIT_JOBS {
        let job = job(workflow, "audit");
        let permissions: Vec<&str> = job
            .lines()
            .skip_while(|l| *l != "    permissions:")
            .skip(1)
            .take_while(|l| l.starts_with("      ") && !l.starts_with("      - "))
            .map(str::trim)
            .filter(|l| !l.starts_with('#'))
            .collect();
        assert!(
            permissions.contains(&"actions: read"),
            "{workflow}: {permissions:?}"
        );
    }
}

#[test]
fn release_yml_grants_the_release_audit_its_lookup() {
    let job = job("release.yml", "custom-ci-gate");
    assert!(
        job.contains("\n    uses: ./.github/workflows/ci-gate.yml\n"),
        "{job}"
    );
    assert!(job.contains("\n      actions: read\n"), "{job}");
}

#[test]
fn the_audit_compares_against_the_baseline_ci_found() {
    for workflow in AUDIT_JOBS {
        let steps = steps(&job(workflow, "audit"));
        let baseline = steps
            .iter()
            .position(|s| s.contains("\nid: audit-baseline\n"))
            .unwrap_or_else(|| panic!("{workflow}: no audit-baseline step"));
        let audit = steps
            .iter()
            .position(|s| s.contains("\nrun: scripts/audit-new-advisories.sh\n"))
            .unwrap_or_else(|| panic!("{workflow}: no audit step"));
        assert!(
            baseline < audit,
            "{workflow}: the baseline must be found before the audit runs"
        );

        let step = &steps[baseline];
        assert!(
            step.contains("\nrun: scripts/audit-baseline.sh\n"),
            "{step}"
        );
        assert!(step.contains("\nGH_TOKEN: ${{ github.token }}\n"), "{step}");
        assert!(
            step.contains(
                "\nAUDIT_DEFAULT_BRANCH: ${{ github.event.repository.default_branch }}\n"
            ),
            "{step}"
        );
        // Only workflows whose runs include the audit: ci.yml, which runs on
        // pushes to main. (ci-gate.yml is called by release.yml, on tags and
        // pull requests, neither of which the baseline looks for.) One that
        // went green without auditing would make every commit it passed a
        // baseline.
        let workflows = step
            .lines()
            .find_map(|l| l.strip_prefix("AUDIT_WORKFLOWS: "))
            .expect("no AUDIT_WORKFLOWS");
        assert_eq!(workflows, "ci.yml", "{workflow}");

        assert!(
            steps[audit].contains("\nAUDIT_BASE: ${{ steps.audit-baseline.outputs.sha }}\n"),
            "{workflow}: the audit must be handed the baseline: {}",
            steps[audit]
        );
    }
}
