use anyhow::{bail, Context, Result};
use std::process::Command;

/// Information fetched from a GitHub issue.
#[derive(Debug)]
pub struct GitHubIssue {
    pub number: u64,
    pub title: String,
    pub body: String,
}

/// Fetch a GitHub issue by number or URL using the `gh` CLI.
pub fn fetch_issue(issue_ref: &str) -> Result<GitHubIssue> {
    // Check if gh is available
    which::which("gh").context(
        "GitHub CLI (gh) is required for --issue. Install it from https://cli.github.com",
    )?;

    // gh issue view works with both issue numbers and URLs
    let output = Command::new("gh")
        .args(["issue", "view", issue_ref, "--json", "number,title,body"])
        .output()
        .context("failed to run gh issue view")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("failed to fetch issue '{issue_ref}': {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value =
        serde_json::from_str(&stdout).context("failed to parse gh output")?;

    let number = json["number"]
        .as_u64()
        .context("issue missing 'number' field")?;
    let title = json["title"]
        .as_str()
        .context("issue missing 'title' field")?
        .to_string();
    let body = json["body"].as_str().unwrap_or("").to_string();

    Ok(GitHubIssue {
        number,
        title,
        body,
    })
}

/// Generate a worktree name from an issue number and title.
/// Format: `<number>-<slugified-title>` (e.g., `42-fix-auth-timeout`)
pub fn issue_to_worktree_name(issue: &GitHubIssue) -> String {
    let slug = slugify(&issue.title, 50);
    format!("{}-{slug}", issue.number)
}

/// Build a prompt string from a GitHub issue.
pub fn issue_to_prompt(issue: &GitHubIssue) -> String {
    let preamble = "You have been assigned the following GitHub issue to work on. \
        Please review the issue, understand what needs to be done, and implement the changes. \
        If this issue is complex or involves changes across multiple files, \
        start by creating a plan before implementing.";

    if issue.body.is_empty() {
        format!(
            "{preamble}\n\nGitHub Issue #{}: {}",
            issue.number, issue.title
        )
    } else {
        format!(
            "{preamble}\n\nGitHub Issue #{}: {}\n\n{}",
            issue.number, issue.title, issue.body
        )
    }
}

/// Slugify a string: lowercase, replace non-alphanumeric with hyphens,
/// collapse consecutive hyphens, trim to max_len, strip trailing hyphens.
fn slugify(s: &str, max_len: usize) -> String {
    let slug: String = s
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();

    // Collapse consecutive hyphens
    let mut result = String::with_capacity(slug.len());
    let mut prev_hyphen = false;
    for c in slug.chars() {
        if c == '-' {
            if !prev_hyphen {
                result.push('-');
            }
            prev_hyphen = true;
        } else {
            result.push(c);
            prev_hyphen = false;
        }
    }

    // Trim to max length and strip leading/trailing hyphens
    let trimmed = if result.len() > max_len {
        &result[..max_len]
    } else {
        &result
    };

    trimmed.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_slugify_simple() {
        assert_eq!(slugify("Fix auth timeout", 50), "fix-auth-timeout");
    }

    #[test]
    fn test_slugify_special_chars() {
        assert_eq!(
            slugify("Fix: user's auth (timeout) bug!", 50),
            "fix-user-s-auth-timeout-bug"
        );
    }

    #[test]
    fn test_slugify_truncate() {
        let long =
            "This is a very long issue title that should be truncated to a reasonable length";
        let result = slugify(long, 30);
        assert!(result.len() <= 30);
        assert!(!result.ends_with('-'));
    }

    #[test]
    fn test_slugify_consecutive_hyphens() {
        assert_eq!(slugify("fix---auth   timeout", 50), "fix-auth-timeout");
    }

    #[test]
    fn test_issue_to_worktree_name() {
        let issue = GitHubIssue {
            number: 42,
            title: "Fix auth timeout".into(),
            body: String::new(),
        };
        assert_eq!(issue_to_worktree_name(&issue), "42-fix-auth-timeout");
    }

    #[test]
    fn test_issue_to_prompt_with_body() {
        let issue = GitHubIssue {
            number: 42,
            title: "Fix auth timeout".into(),
            body: "The login page times out after 30s".into(),
        };
        let prompt = issue_to_prompt(&issue);
        assert!(prompt.contains("GitHub Issue #42"));
        assert!(prompt.contains("Fix auth timeout"));
        assert!(prompt.contains("The login page times out"));
    }

    #[test]
    fn test_issue_to_prompt_no_body() {
        let issue = GitHubIssue {
            number: 42,
            title: "Fix auth timeout".into(),
            body: String::new(),
        };
        let prompt = issue_to_prompt(&issue);
        assert!(prompt.contains("assigned the following GitHub issue"));
        assert!(prompt.contains("GitHub Issue #42: Fix auth timeout"));
    }
}
