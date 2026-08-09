use anyhow::Result;
use std::collections::HashMap;
use std::io::Write as _;
use std::path::Path;

use crate::agent_hooks;
use crate::git;
use crate::state::{Workspace, WorkspaceState};

/// How often `--watch` refreshes.
const WATCH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);

/// Everything the dashboard needs to draw one workspace row. Gathered off the
/// main thread so the git probes for different workspaces overlap.
struct Row {
    workspace_name: String,
    /// The worktree directory is gone — nothing else was probed.
    missing: bool,
    dirty: bool,
    commit_info: String,
    agent_infos: Vec<(String, agent_hooks::AgentStatusInfo)>,
}

/// Display a status dashboard of all active workspaces.
pub fn run(state: &WorkspaceState, state_path: &Path, watch: bool) -> Result<()> {
    if !watch {
        return render_dashboard(state);
    }

    // Re-read state each tick so workspaces started or finished in another tab
    // appear and disappear. A transient read failure (state.toml being
    // rewritten concurrently) just keeps the last good snapshot on screen.
    let mut snapshot = state.clone();
    loop {
        print!("\x1b[2J\x1b[H"); // clear screen
        render_dashboard(&snapshot)?;
        std::io::stdout().flush()?;

        std::thread::sleep(WATCH_INTERVAL);

        if let Ok(mut next) = WorkspaceState::load_from(state_path) {
            next.prune_stale();
            snapshot = next;
        }
    }
}

/// Probe every workspace concurrently.
///
/// Each row costs a couple of git subprocesses, and they are independent and
/// IO-bound, so running them serially made a refresh take the sum of every
/// repo's `git status`. Work is chunked across at most `available_parallelism`
/// threads; rows come back in input order.
fn probe_workspaces(
    workspaces: &[Workspace],
    main_branches: &HashMap<&str, Option<String>>,
) -> Vec<Row> {
    let threads = std::thread::available_parallelism()
        .map(|p| p.get())
        .unwrap_or(4)
        .min(workspaces.len())
        .max(1);
    let chunk_size = workspaces.len().div_ceil(threads);

    std::thread::scope(|scope| {
        let handles: Vec<_> = workspaces
            .chunks(chunk_size)
            .map(|chunk| {
                scope.spawn(move || {
                    chunk
                        .iter()
                        .map(|ws| probe_one(ws, main_branches))
                        .collect::<Vec<Row>>()
                })
            })
            .collect();

        handles
            .into_iter()
            .flat_map(|h| h.join().expect("status probe thread panicked"))
            .collect()
    })
}

/// Gather the display data for a single workspace.
fn probe_one(ws: &Workspace, main_branches: &HashMap<&str, Option<String>>) -> Row {
    let workspace_name = format!("{}/{}", ws.project, ws.name);
    let worktree = Path::new(&ws.worktree_path);
    let source = Path::new(&ws.source_path);

    if !worktree.exists() {
        return Row {
            workspace_name,
            missing: true,
            dirty: false,
            commit_info: String::new(),
            agent_infos: Vec::new(),
        };
    }

    // Commit count vs main. One `rev-list --count` answers both "are there
    // commits?" and "how many?" — asking twice ran the identical command.
    let commit_info = match main_branches
        .get(ws.source_path.as_str())
        .and_then(|m| m.as_ref())
    {
        Some(main_branch) => match git::commit_count(source, &ws.branch, main_branch) {
            Ok(0) => "no commits".to_string(),
            Ok(1) => "1 commit".to_string(),
            Ok(count) => format!("{count} commits"),
            Err(_) => "unknown".to_string(),
        },
        None => "unknown".to_string(),
    };

    Row {
        workspace_name,
        missing: false,
        dirty: git::has_uncommitted_changes(worktree).unwrap_or(false),
        commit_info,
        agent_infos: agent_hooks::read_all_status_infos(&ws.project, &ws.name),
    }
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

    // Every workspace in a project shares one source repo, so detect the main
    // branch once per repo rather than once per workspace.
    let mut main_branches: HashMap<&str, Option<String>> = HashMap::new();
    for ws in workspaces {
        main_branches
            .entry(ws.source_path.as_str())
            .or_insert_with(|| git::detect_main_branch(Path::new(&ws.source_path)).ok());
    }

    for row in probe_workspaces(workspaces, &main_branches) {
        let Row {
            workspace_name,
            missing,
            dirty,
            commit_info,
            agent_infos,
        } = row;

        if missing {
            println!("  {:<30} \x1b[31m✗ missing\x1b[0m", workspace_name);
            continue;
        }

        let (git_label, git_color) = if dirty {
            ("⚠ dirty", "\x1b[33m")
        } else {
            ("✓ clean", "\x1b[32m")
        };

        let (agent_label, agent_color, activity) = if agent_infos.is_empty() {
            ("unknown".to_string(), "", String::new())
        } else if agent_infos.len() == 1 {
            let (agent_name, info) = &agent_infos[0];
            if info.is_stale() {
                (
                    format!("{agent_name}: idle?"),
                    "\x1b[33m",
                    "no activity for 5+ minutes".to_string(),
                )
            } else {
                let (label, color) = status_display(&info.status);
                let act = activity_text(info);
                (format!("{agent_name}: {label}"), color, act)
            }
        } else {
            // Multiple agents — show each
            let parts: Vec<String> = agent_infos
                .iter()
                .map(|(agent_name, info)| {
                    if info.is_stale() {
                        format!("{agent_name}:idle?")
                    } else {
                        let (label, _) = status_display(&info.status);
                        format!("{agent_name}:{label}")
                    }
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
