use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;

use super::{CheckConclusion, CheckRun, ChecksStatus, Forge, PrInfo};

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

        let output = Command::new("gh")
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
            .current_dir(repo_path)
            .output()
            .context("failed to run gh pr create")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("failed to create PR: {}", stderr.trim());
        }

        parse_pr_json(&output.stdout)
    }

    fn merge_pr(&self, repo_path: &Path, branch: &str) -> Result<()> {
        check_gh()?;

        let output = Command::new("gh")
            .args(["pr", "merge", branch, "--merge", "--delete-branch"])
            .current_dir(repo_path)
            .output()
            .context("failed to run gh pr merge")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("failed to merge PR: {}", stderr.trim());
        }

        Ok(())
    }

    fn pr_for_branch(&self, repo_path: &Path, branch: &str) -> Result<Option<PrInfo>> {
        check_gh()?;

        // Use `gh pr list` with --head filter and --state open to find only
        // open PRs for this branch. `gh pr view` returns the most recent PR
        // regardless of state (open/closed/merged), which would incorrectly
        // detect a closed PR from a previous workspace that reused the branch name.
        let output = Command::new("gh")
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
            .current_dir(repo_path)
            .output()
            .context("failed to run gh pr list")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("failed to check for existing PR: {}", stderr.trim());
        }

        let json: serde_json::Value =
            serde_json::from_slice(&output.stdout).context("failed to parse gh output as JSON")?;

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

        let output = Command::new("gh")
            .args(["pr", "checks", branch, "--json", "name,state"])
            .current_dir(repo_path)
            .output()
            .context("failed to run gh pr checks")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // If there are no checks configured, gh returns an error
            if stderr.contains("no checks") || stderr.contains("no status checks") {
                return Ok(ChecksStatus { checks: Vec::new() });
            }
            bail!("failed to get PR checks: {}", stderr.trim());
        }

        let json: serde_json::Value =
            serde_json::from_slice(&output.stdout).context("failed to parse gh output as JSON")?;

        let arr = json
            .as_array()
            .context("expected JSON array from gh pr checks")?;

        let checks = arr
            .iter()
            .map(|item| {
                let name = item["name"].as_str().unwrap_or("unknown").to_string();
                let state = item["state"].as_str().unwrap_or("");
                let conclusion = match state {
                    "SUCCESS" => CheckConclusion::Pass,
                    "FAILURE" | "ERROR" => CheckConclusion::Fail,
                    "SKIPPED" | "NEUTRAL" => CheckConclusion::Skipped,
                    _ => CheckConclusion::Pending,
                };
                CheckRun { name, conclusion }
            })
            .collect();

        Ok(ChecksStatus { checks })
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
