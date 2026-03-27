use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;

use super::{Forge, PrInfo};

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

        let output = Command::new("gh")
            .args(["pr", "view", branch, "--json", "number,url"])
            .current_dir(repo_path)
            .output()
            .context("failed to run gh pr view")?;

        if !output.status.success() {
            // gh pr view returns non-zero when no PR exists for the branch
            return Ok(None);
        }

        Ok(Some(parse_pr_json(&output.stdout)?))
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

    let number = json["number"]
        .as_u64()
        .context("PR response missing 'number' field")?;
    let url = json["url"]
        .as_str()
        .context("PR response missing 'url' field")?
        .to_string();

    Ok(PrInfo { number, url })
}
