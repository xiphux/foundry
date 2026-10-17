use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use super::{CheckConclusion, CheckRun, ChecksStatus, Forge, PrInfo};
use crate::process::{CommandFailed, run_checked};

pub struct GitHubForge;

impl Forge for GitHubForge {
    fn create_pr(
        &self,
        repo_path: &Path,
        branch: &str,
        base: &str,
        title: &str,
        body: &str,
    ) -> Result<PrInfo> {
        check_gh()?;

        let output = run_checked(
            Command::new("gh")
                .args([
                    "pr",
                    "create",
                    "--head",
                    branch,
                    "--base",
                    base,
                    "--title",
                    title,
                    "--body",
                    body,
                    "--json",
                    "number,url",
                ])
                .current_dir(repo_path),
            "failed to create PR",
        )?;

        parse_pr_json(output.as_bytes())
    }

    fn merge_pr(&self, repo_path: &Path, branch: &str) -> Result<()> {
        check_gh()?;

        // Not `--delete-branch`: that also deletes the *local* branch, which
        // git refuses while the workspace's worktree has it checked out — and
        // gh then exits non-zero although the PR is already merged. The local
        // branch goes in cleanup, once the worktree is removed; `finish`
        // deletes the remote one.
        run_checked(
            Command::new("gh")
                .args(["pr", "merge", branch, "--merge"])
                .current_dir(repo_path),
            "failed to merge PR",
        )?;
        Ok(())
    }

    fn pr_for_branch(&self, repo_path: &Path, branch: &str) -> Result<Option<PrInfo>> {
        check_gh()?;

        // Use `gh pr list` with --head filter and --state open to find only
        // open PRs for this branch. `gh pr view` returns the most recent PR
        // regardless of state (open/closed/merged), which would incorrectly
        // detect a closed PR from a previous workspace that reused the branch name.
        let output = run_checked(
            Command::new("gh")
                .args([
                    "pr",
                    "list",
                    "--head",
                    branch,
                    "--state",
                    "open",
                    "--json",
                    "number,url",
                    "--limit",
                    "1",
                ])
                .current_dir(repo_path),
            "failed to check for existing PR",
        )?;

        let json: serde_json::Value =
            serde_json::from_str(&output).context("failed to parse gh output as JSON")?;

        let arr = json
            .as_array()
            .context("expected JSON array from gh pr list")?;
        if arr.is_empty() {
            return Ok(None);
        }

        Ok(Some(parse_pr_json_value(&arr[0])?))
    }
    fn pr_checks(&self, repo_path: &Path, branch: &str) -> Result<ChecksStatus> {
        check_gh()?;

        let output = match run_checked(
            Command::new("gh")
                .args(["pr", "checks", branch, "--json", "name,state,bucket"])
                .current_dir(repo_path),
            "failed to get PR checks",
        ) {
            Ok(output) => output,
            // gh exits non-zero for a PR with no checks configured. Matched on
            // its wording, which is all it offers to tell this apart.
            Err(e)
                if e.downcast_ref::<CommandFailed>().is_some_and(|f| {
                    f.stderr.contains("no checks") || f.stderr.contains("no status checks")
                }) =>
            {
                return Ok(ChecksStatus { checks: Vec::new() });
            }
            Err(e) => return Err(e),
        };

        let json: serde_json::Value =
            serde_json::from_str(&output).context("failed to parse gh output as JSON")?;

        let arr = json
            .as_array()
            .context("expected JSON array from gh pr checks")?;

        let checks = arr
            .iter()
            .map(|item| {
                let name = item["name"].as_str().unwrap_or("unknown").to_string();
                let conclusion = check_conclusion(
                    item["bucket"].as_str(),
                    item["state"].as_str().unwrap_or(""),
                );
                CheckRun { name, conclusion }
            })
            .collect();

        Ok(ChecksStatus { checks })
    }
}

/// Classify one check. `bucket` is gh's own summary of `state` and the field
/// meant for this; `state` is the fallback, and lists the finished-but-not-
/// passed states explicitly — treating every unlisted state as pending made a
/// cancelled or timed-out run read as "still running" when `finish` asked.
fn check_conclusion(bucket: Option<&str>, state: &str) -> CheckConclusion {
    match bucket {
        Some("pass") => return CheckConclusion::Pass,
        Some("fail" | "cancel") => return CheckConclusion::Fail,
        Some("skipping") => return CheckConclusion::Skipped,
        Some("pending") => return CheckConclusion::Pending,
        _ => {}
    }
    match state {
        "SUCCESS" => CheckConclusion::Pass,
        "FAILURE" | "ERROR" | "CANCELLED" | "TIMED_OUT" | "ACTION_REQUIRED" | "STARTUP_FAILURE"
        | "STALE" => CheckConclusion::Fail,
        "SKIPPED" | "NEUTRAL" => CheckConclusion::Skipped,
        _ => CheckConclusion::Pending,
    }
}

fn check_gh() -> Result<()> {
    which::which("gh").context(
        "GitHub CLI (gh) is required for PR commands. Install it from https://cli.github.com",
    )?;
    Ok(())
}

fn parse_pr_json(stdout: &[u8]) -> Result<PrInfo> {
    let json: serde_json::Value =
        serde_json::from_slice(stdout).context("failed to parse gh output as JSON")?;
    parse_pr_json_value(&json)
}

fn parse_pr_json_value(json: &serde_json::Value) -> Result<PrInfo> {
    let number = json["number"]
        .as_u64()
        .context("PR response missing 'number' field")?;
    let url = json["url"]
        .as_str()
        .context("PR response missing 'url' field")?
        .to_string();

    Ok(PrInfo { number, url })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_conclusion_prefers_the_bucket() {
        assert_eq!(check_conclusion(Some("pass"), ""), CheckConclusion::Pass);
        assert_eq!(
            check_conclusion(Some("cancel"), "CANCELLED"),
            CheckConclusion::Fail
        );
        assert_eq!(
            check_conclusion(Some("skipping"), ""),
            CheckConclusion::Skipped
        );
        assert_eq!(
            check_conclusion(Some("pending"), "QUEUED"),
            CheckConclusion::Pending
        );
    }

    #[test]
    fn check_conclusion_counts_finished_unsuccessful_states_as_failed() {
        for state in [
            "CANCELLED",
            "TIMED_OUT",
            "ACTION_REQUIRED",
            "STARTUP_FAILURE",
        ] {
            assert_eq!(
                check_conclusion(None, state),
                CheckConclusion::Fail,
                "{state}"
            );
        }
        assert_eq!(
            check_conclusion(None, "IN_PROGRESS"),
            CheckConclusion::Pending
        );
    }
}
