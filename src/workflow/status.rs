use anyhow::Result;
use std::path::Path;

use crate::agent_hooks;
use crate::git;
use crate::state::WorkspaceState;

/// Display a status dashboard of all active workspaces.
pub fn run(state: &WorkspaceState, watch: bool) -> Result<()> {
    if watch {
        loop {
            print!("\x1b[2J\x1b[H"); // clear screen
            render_dashboard(state)?;
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    } else {
        render_dashboard(state)?;
    }
    Ok(())
}

/// Render the status dashboard once.
fn render_dashboard(state: &WorkspaceState) -> Result<()> {
    let workspaces = state.list();

    if workspaces.is_empty() {
        println!("No active workspaces.");
        return Ok(());
    }

    // Print header
    println!(
        "  {:<30} {:<10} {:<14} AGENT",
        "WORKSPACE", "GIT", "COMMITS"
    );
    println!("  {}", "\u{2500}".repeat(80));

    for ws in workspaces {
        let worktree = Path::new(&ws.worktree_path);
        let source = Path::new(&ws.source_path);
        let workspace_name = format!("{}/{}", ws.project, ws.name);

        // Check if worktree still exists
        if !worktree.exists() {
            println!("  {:<30} \x1b[31m✗ missing\x1b[0m", workspace_name);
            continue;
        }

        // Git status
        let dirty = git::has_uncommitted_changes(worktree).unwrap_or(false);
        let (git_label, git_color) = if dirty {
            ("⚠ dirty", "\x1b[33m")
        } else {
            ("✓ clean", "\x1b[32m")
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

        // Agent status (may have multiple agents per workspace)
        let agent_infos = agent_hooks::read_all_status_infos(&ws.project, &ws.name);
        let (agent_label, agent_color, activity) = if agent_infos.is_empty() {
            ("unknown".to_string(), "", String::new())
        } else if agent_infos.len() == 1 {
            let (agent_name, info) = &agent_infos[0];
            let (label, color) = status_display(&info.status);
            let act = activity_text(info);
            (format!("{agent_name}: {label}"), color, act)
        } else {
            // Multiple agents — show each
            let parts: Vec<String> = agent_infos
                .iter()
                .map(|(agent_name, info)| {
                    let (label, _) = status_display(&info.status);
                    format!("{agent_name}:{label}")
                })
                .collect();
            // Use the most urgent color
            let color = if agent_infos
                .iter()
                .any(|(_, i)| matches!(i.status, agent_hooks::AgentStatus::WaitingPermission))
            {
                "\x1b[31m"
            } else if agent_infos
                .iter()
                .any(|(_, i)| matches!(i.status, agent_hooks::AgentStatus::Working))
            {
                "\x1b[34m"
            } else {
                "\x1b[33m"
            };
            // Activity from the first non-empty result
            let act = agent_infos
                .iter()
                .map(|(_, i)| activity_text(i))
                .find(|s| !s.is_empty())
                .unwrap_or_default();
            (parts.join(" "), color, act)
        };

        // Pad visible text first, then wrap with color codes so ANSI escapes
        // don't interfere with column widths.
        let git_padded = format!("{:<10}", git_label);
        println!(
            "  {:<30} {}{}\x1b[0m {:<14} {}{}\x1b[0m",
            workspace_name, git_color, git_padded, commit_info, agent_color, agent_label
        );

        // Activity on a second indented line (only if non-empty)
        if !activity.is_empty() {
            println!("    \x1b[90mActivity:\x1b[0m {activity}");
        }
    }

    Ok(())
}

/// Map agent status to display label and ANSI color.
fn status_display(status: &agent_hooks::AgentStatus) -> (&'static str, &'static str) {
    match status {
        agent_hooks::AgentStatus::Working => ("working", "\x1b[34m"),
        agent_hooks::AgentStatus::Idle => ("idle", "\x1b[33m"),
        agent_hooks::AgentStatus::WaitingPermission => ("waiting", "\x1b[31m"),
        agent_hooks::AgentStatus::Error => ("error", "\x1b[31m"),
        agent_hooks::AgentStatus::Offline => ("offline", "\x1b[90m"),
        agent_hooks::AgentStatus::Unknown => ("unknown", ""),
    }
}

/// Build an activity string from rich status info.
fn activity_text(info: &agent_hooks::AgentStatusInfo) -> String {
    match info.status {
        agent_hooks::AgentStatus::Working => info
            .last_tool
            .as_deref()
            .map(|t| {
                if t.len() > 50 {
                    format!("{}...", &t[..47])
                } else {
                    t.to_string()
                }
            })
            .unwrap_or_default(),
        agent_hooks::AgentStatus::Idle => info
            .last_message
            .as_deref()
            .map(|m| {
                let truncated = if m.len() > 60 {
                    format!("{}...", &m[..57])
                } else {
                    m.to_string()
                };
                format!("\"{truncated}\"")
            })
            .unwrap_or_default(),
        agent_hooks::AgentStatus::Error => info
            .error
            .clone()
            .unwrap_or_else(|| "unknown error".to_string()),
        _ => String::new(),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_commit_count_no_commits() {
        let dir = tempfile::TempDir::new().unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "--allow-empty", "-m", "initial"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["branch", "-M", "main"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["branch", "feature"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert_eq!(commit_count(dir.path(), "feature", "main"), 0);
    }

    #[test]
    fn test_commit_count_with_commits() {
        let dir = tempfile::TempDir::new().unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "--allow-empty", "-m", "initial"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["branch", "-M", "main"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["checkout", "-b", "feature"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "--allow-empty", "-m", "feat 1"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "--allow-empty", "-m", "feat 2"])
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert_eq!(commit_count(dir.path(), "feature", "main"), 2);
    }

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
