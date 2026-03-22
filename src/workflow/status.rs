use anyhow::Result;
use std::path::Path;

use crate::agent_hooks;
use crate::git;
use crate::state::WorkspaceState;

/// Display a status dashboard of all active workspaces.
pub fn run(state: &WorkspaceState) -> Result<()> {
    let workspaces = state.list();

    if workspaces.is_empty() {
        println!("No active workspaces.");
        return Ok(());
    }

    // Print header
    println!(
        "  {:<30} {:<10} {:<14} {:<24} LAST ACTIVITY",
        "WORKSPACE", "STATUS", "COMMITS", "AGENT"
    );
    println!("  {}", "-".repeat(90));

    for ws in workspaces {
        let worktree = Path::new(&ws.worktree_path);
        let source = Path::new(&ws.source_path);

        // Check if worktree still exists
        if !worktree.exists() {
            println!(
                "  {:<30} \x1b[31m{:<10}\x1b[0m",
                format!("{}/{}", ws.project, ws.name),
                "✗ missing"
            );
            continue;
        }

        // Git status
        let dirty = git::has_uncommitted_changes(worktree).unwrap_or(false);
        let status_str = if dirty {
            format!("\x1b[33m{:<10}\x1b[0m", "⚠ dirty")
        } else {
            format!("\x1b[32m{:<10}\x1b[0m", "✓ clean")
        };

        // Commit count vs main
        let commit_info = if let Ok(main_branch) = git::detect_main_branch(source) {
            match git::branch_has_commits(source, &ws.branch, &main_branch) {
                Ok(true) => {
                    let count = commit_count(source, &ws.branch, &main_branch);
                    if count == 1 {
                        "1 commit".to_string()
                    } else {
                        format!("{count} commits")
                    }
                }
                Ok(false) => "no commits".to_string(),
                Err(_) => "unknown".to_string(),
            }
        } else {
            "unknown".to_string()
        };

        // Agent status
        let agent_status = agent_hooks::read_status(&ws.project, &ws.name);
        let agent_str = agent_status.colored_label();

        // Time since last commit
        let time_ago = match git::last_commit_timestamp(worktree) {
            Ok(Some(ts)) => format_time_ago(ts),
            _ => "-".to_string(),
        };

        let workspace_name = format!("{}/{}", ws.project, ws.name);
        println!(
            "  {:<30} {} {:<14} {:<24} {}",
            workspace_name, status_str, commit_info, agent_str, time_ago
        );
    }

    Ok(())
}

/// Get the number of commits a branch has beyond base.
fn commit_count(repo_path: &Path, branch: &str, base: &str) -> u64 {
    let range = format!("{base}..{branch}");
    std::process::Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["rev-list", "--count", &range])
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse().ok())
        .unwrap_or(0)
}

/// Format a Unix timestamp as a human-readable "X ago" string.
fn format_time_ago(timestamp: i64) -> String {
    let now = chrono::Utc::now().timestamp();
    let diff = now - timestamp;

    if diff < 0 {
        return "just now".to_string();
    }

    let seconds = diff as u64;
    let minutes = seconds / 60;
    let hours = minutes / 60;
    let days = hours / 24;

    if days > 0 {
        if days == 1 {
            "1d ago".to_string()
        } else {
            format!("{days}d ago")
        }
    } else if hours > 0 {
        if hours == 1 {
            "1h ago".to_string()
        } else {
            format!("{hours}h ago")
        }
    } else if minutes > 0 {
        if minutes == 1 {
            "1m ago".to_string()
        } else {
            format!("{minutes}m ago")
        }
    } else {
        "just now".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_time_ago() {
        let now = chrono::Utc::now().timestamp();
        assert_eq!(format_time_ago(now), "just now");
        assert_eq!(format_time_ago(now - 30), "just now");
        assert_eq!(format_time_ago(now - 120), "2m ago");
        assert_eq!(format_time_ago(now - 3600), "1h ago");
        assert_eq!(format_time_ago(now - 7200), "2h ago");
        assert_eq!(format_time_ago(now - 86400), "1d ago");
        assert_eq!(format_time_ago(now - 259200), "3d ago");
    }
}
