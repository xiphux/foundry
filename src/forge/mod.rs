pub mod github;

use anyhow::Result;

/// Identifies which forge platform a remote points to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForgeKind {
    GitHub,
    GitLab,
}

/// Result of creating a pull/merge request.
#[derive(Debug)]
pub struct PrInfo {
    /// The PR/MR number.
    pub number: u64,
    /// The URL of the PR/MR.
    pub url: String,
}

/// Trait for forge (GitHub, GitLab, etc.) operations.
/// Analogous to `TerminalBackend` — implementations shell out to
/// CLI tools (`gh`, `glab`) for the actual operations.
pub trait Forge {
    /// Create a pull/merge request for the given branch.
    fn create_pr(
        &self,
        repo_path: &std::path::Path,
        branch: &str,
        base: &str,
        title: &str,
        body: &str,
    ) -> Result<PrInfo>;

    /// Merge a pull/merge request by branch name.
    fn merge_pr(&self, repo_path: &std::path::Path, branch: &str) -> Result<()>;

    /// Get the PR/MR info for a branch, if one exists.
    fn pr_for_branch(&self, repo_path: &std::path::Path, branch: &str) -> Result<Option<PrInfo>>;
}

/// Detect the forge kind from a remote URL.
///
/// Matches on the hostname portion of HTTPS URLs and SSH URLs.
pub fn detect_forge_from_url(url: &str) -> Option<ForgeKind> {
    let lower = url.to_lowercase();
    if lower.contains("github.com") {
        Some(ForgeKind::GitHub)
    } else if lower.contains("gitlab.com") {
        Some(ForgeKind::GitLab)
    } else {
        None
    }
}

/// Resolve which remote to use for PR operations.
///
/// Logic:
/// 1. If the user configured `pr_remote`, use that.
/// 2. If there's exactly one remote, use it.
/// 3. Otherwise, default to "origin".
pub fn resolve_pr_remote(configured: Option<&str>, remotes: &[String]) -> String {
    if let Some(r) = configured {
        return r.to_string();
    }
    if remotes.len() == 1 {
        return remotes[0].clone();
    }
    "origin".to_string()
}

/// Detect the forge for a repository by inspecting the push remote URL.
///
/// Returns the forge implementation and the resolved remote name.
pub fn detect_forge(
    repo_path: &std::path::Path,
    configured_remote: Option<&str>,
) -> Result<(Box<dyn Forge>, String)> {
    let remotes = crate::git::list_remotes(repo_path)?;
    let remote = resolve_pr_remote(configured_remote, &remotes);

    let url = crate::git::remote_url(repo_path, &remote).map_err(|_| {
        anyhow::anyhow!(
            "remote '{remote}' not found. Configure pr_remote in your .foundry.toml \
             or add a remote with `git remote add {remote} <url>`."
        )
    })?;

    let kind = detect_forge_from_url(&url).ok_or_else(|| {
        anyhow::anyhow!(
            "could not detect forge for remote '{remote}' (URL: {url}). \
             PR commands currently support GitHub (github.com) remotes."
        )
    })?;

    match kind {
        ForgeKind::GitHub => Ok((Box::new(github::GitHubForge), remote)),
        ForgeKind::GitLab => anyhow::bail!(
            "GitLab support is not yet implemented. \
             See https://github.com/xiphux/foundry/issues/35"
        ),
    }
}
