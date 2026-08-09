use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::config;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_event_started_serializes() {
        let event = HistoryEvent::started("myapp", "fix-auth", "fix-auth", Some("42"));
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"event\":\"started\""));
        assert!(json.contains("\"from_issue\":\"42\""));
        // Should not contain None fields
        assert!(!json.contains("commits"));
        assert!(!json.contains("archived_as"));
    }

    #[test]
    fn history_event_finished_serializes() {
        let event = HistoryEvent::finished("myapp", "fix-auth", "fix-auth", 3, "ff-only");
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"commits\":3"));
        assert!(json.contains("\"merge_strategy\":\"ff-only\""));
    }

    #[test]
    fn history_event_discarded_with_archive() {
        let event = HistoryEvent::discarded(
            "myapp",
            "experiment",
            "experiment",
            5,
            Some("archive/experiment-20260322"),
        );
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"archived_as\":\"archive/experiment-20260322\""));
        assert!(json.contains("\"commits\":5"));
    }

    #[test]
    fn history_event_discarded_without_archive() {
        let event = HistoryEvent::discarded("myapp", "experiment", "experiment", 0, None);
        let json = serde_json::to_string(&event).unwrap();
        assert!(!json.contains("archived_as"));
    }

    #[test]
    fn history_event_restored_serializes() {
        let event = HistoryEvent::restored(
            "myapp",
            "experiment",
            "experiment",
            "archive/experiment-20260322",
        );
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"from_branch\":\"archive/experiment-20260322\""));
    }

    #[test]
    fn history_event_roundtrip() {
        let event = HistoryEvent::started("myapp", "feat", "feat", None);
        let json = serde_json::to_string(&event).unwrap();
        let parsed: HistoryEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.event, "started");
        assert_eq!(parsed.project, "myapp");
        assert_eq!(parsed.name, "feat");
    }

    #[test]
    fn history_event_pr_created_serializes() {
        let event = HistoryEvent::pr_created(
            "myapp",
            "fix-auth",
            "fix-auth",
            42,
            "https://github.com/user/repo/pull/42",
        );
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"event\":\"pr_created\""));
        assert!(json.contains("\"pr_number\":42"));
        assert!(json.contains("\"pr_url\":\"https://github.com/user/repo/pull/42\""));
    }

    /// Write `count` events to a temp log, named "ws-0".."ws-N" in order.
    fn write_log(count: usize) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("history.jsonl");
        let mut out = String::new();
        for i in 0..count {
            let event = HistoryEvent::started("proj", &format!("ws-{i}"), "branch", None);
            out.push_str(&serde_json::to_string(&event).unwrap());
            out.push('\n');
        }
        std::fs::write(&path, out).unwrap();
        (dir, path)
    }

    #[test]
    fn read_recent_returns_newest_first() {
        let (_dir, path) = write_log(5);
        let events = read_recent_from(&path, 3).unwrap();
        let names: Vec<&str> = events.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["ws-4", "ws-3", "ws-2"]);
    }

    #[test]
    fn read_recent_handles_missing_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let events = read_recent_from(&dir.path().join("nope.jsonl"), 10).unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn read_recent_limit_zero_returns_empty() {
        let (_dir, path) = write_log(5);
        assert!(read_recent_from(&path, 0).unwrap().is_empty());
    }

    #[test]
    fn read_recent_limit_exceeding_file_returns_all() {
        let (_dir, path) = write_log(3);
        let events = read_recent_from(&path, 100).unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[2].name, "ws-0");
    }

    /// The window must grow past TAIL_WINDOW rather than truncating the result.
    #[test]
    fn read_recent_grows_window_past_first_read() {
        // Each event is ~150 bytes, so 2000 of them far exceed the 64 KiB window.
        let (_dir, path) = write_log(2000);
        let events = read_recent_from(&path, 1500).unwrap();
        assert_eq!(events.len(), 1500);
        assert_eq!(events[0].name, "ws-1999");
        assert_eq!(events[1499].name, "ws-500");
    }

    /// A record split by the window boundary must not surface as a partial event.
    #[test]
    fn read_recent_discards_record_split_by_window() {
        let (_dir, path) = write_log(1000);
        // Ask for few events so the first window is used and its leading line
        // is almost certainly a partial record.
        let events = read_recent_from(&path, 5).unwrap();
        assert_eq!(events.len(), 5);
        assert_eq!(events[0].name, "ws-999");
        assert!(events.iter().all(|e| e.event == "started"));
    }

    #[test]
    fn read_recent_skips_corrupt_lines() {
        let (_dir, path) = write_log(4);
        let mut content = std::fs::read_to_string(&path).unwrap();
        content.push_str("{ not valid json\n\n");
        std::fs::write(&path, content).unwrap();

        let events = read_recent_from(&path, 3).unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].name, "ws-3");
    }

    #[test]
    fn read_recent_handles_empty_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("history.jsonl");
        std::fs::write(&path, "").unwrap();
        assert!(read_recent_from(&path, 10).unwrap().is_empty());
    }

    #[test]
    fn history_event_pr_merged_serializes() {
        let event = HistoryEvent::pr_merged("myapp", "fix-auth", "fix-auth", 42);
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"event\":\"pr_merged\""));
        assert!(json.contains("\"pr_number\":42"));
    }
}

/// A workspace lifecycle event recorded in the history log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEvent {
    pub event: String,
    pub project: String,
    pub name: String,
    pub branch: String,
    pub timestamp: DateTime<Utc>,

    // Optional metadata per event type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_issue: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commits: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merge_strategy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived_as: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_number: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr_url: Option<String>,
}

impl HistoryEvent {
    pub fn started(project: &str, name: &str, branch: &str, from_issue: Option<&str>) -> Self {
        Self {
            event: "started".into(),
            project: project.into(),
            name: name.into(),
            branch: branch.into(),
            timestamp: Utc::now(),
            from_issue: from_issue.map(|s| s.into()),
            commits: None,
            merge_strategy: None,
            archived_as: None,
            from_branch: None,
            pr_number: None,
            pr_url: None,
        }
    }

    pub fn finished(
        project: &str,
        name: &str,
        branch: &str,
        commits: u64,
        merge_strategy: &str,
    ) -> Self {
        Self {
            event: "finished".into(),
            project: project.into(),
            name: name.into(),
            branch: branch.into(),
            timestamp: Utc::now(),
            from_issue: None,
            commits: Some(commits),
            merge_strategy: Some(merge_strategy.into()),
            archived_as: None,
            from_branch: None,
            pr_number: None,
            pr_url: None,
        }
    }

    pub fn discarded(
        project: &str,
        name: &str,
        branch: &str,
        commits: u64,
        archived_as: Option<&str>,
    ) -> Self {
        Self {
            event: "discarded".into(),
            project: project.into(),
            name: name.into(),
            branch: branch.into(),
            timestamp: Utc::now(),
            from_issue: None,
            commits: Some(commits),
            merge_strategy: None,
            archived_as: archived_as.map(|s| s.into()),
            from_branch: None,
            pr_number: None,
            pr_url: None,
        }
    }

    pub fn restored(project: &str, name: &str, branch: &str, from_branch: &str) -> Self {
        Self {
            event: "restored".into(),
            project: project.into(),
            name: name.into(),
            branch: branch.into(),
            timestamp: Utc::now(),
            from_issue: None,
            commits: None,
            merge_strategy: None,
            archived_as: None,
            from_branch: Some(from_branch.into()),
            pr_number: None,
            pr_url: None,
        }
    }

    pub fn pr_created(
        project: &str,
        name: &str,
        branch: &str,
        pr_number: u64,
        pr_url: &str,
    ) -> Self {
        Self {
            event: "pr_created".into(),
            project: project.into(),
            name: name.into(),
            branch: branch.into(),
            timestamp: Utc::now(),
            from_issue: None,
            commits: None,
            merge_strategy: None,
            archived_as: None,
            from_branch: None,
            pr_number: Some(pr_number),
            pr_url: Some(pr_url.into()),
        }
    }

    pub fn pr_merged(project: &str, name: &str, branch: &str, pr_number: u64) -> Self {
        Self {
            event: "pr_merged".into(),
            project: project.into(),
            name: name.into(),
            branch: branch.into(),
            timestamp: Utc::now(),
            from_issue: None,
            commits: None,
            merge_strategy: None,
            archived_as: None,
            from_branch: None,
            pr_number: Some(pr_number),
            pr_url: None,
        }
    }
}

/// Get the path to the history log file.
fn history_path() -> Result<std::path::PathBuf> {
    let dir = config::foundry_dir()?;
    Ok(dir.join("history.jsonl"))
}

/// Append a history event to the log.
pub fn record(event: &HistoryEvent) -> Result<()> {
    let path = history_path()?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("failed to open history log at {}", path.display()))?;

    let line = serde_json::to_string(event).context("failed to serialize history event")?;
    writeln!(file, "{line}")?;

    Ok(())
}

/// How much of the tail to read on the first attempt, in bytes. Doubles until
/// enough events are found or the whole file has been read.
const TAIL_WINDOW: u64 = 64 * 1024;

/// Read the most recent history events, up to `limit`.
pub fn read_recent(limit: usize) -> Result<Vec<HistoryEvent>> {
    read_recent_from(&history_path()?, limit)
}

/// Read the most recent events from a specific log file (for testability).
///
/// The log is append-only and never rotated, so it grows without bound. Reading
/// it front-to-back to show the last 20 entries meant parsing every event ever
/// recorded. Instead, seek to a window at the end of the file and parse
/// backward from there, growing the window only if it held too few events.
fn read_recent_from(path: &Path, limit: usize) -> Result<Vec<HistoryEvent>> {
    if limit == 0 || !path.exists() {
        return Ok(Vec::new());
    }

    let mut file = File::open(path)
        .with_context(|| format!("failed to open history log at {}", path.display()))?;
    let file_len = file
        .metadata()
        .with_context(|| format!("failed to stat history log at {}", path.display()))?
        .len();

    let mut window = TAIL_WINDOW;
    loop {
        let start = file_len.saturating_sub(window);
        file.seek(SeekFrom::Start(start))?;

        let mut buf = Vec::with_capacity((file_len - start) as usize);
        file.read_to_end(&mut buf)?;

        let text = String::from_utf8_lossy(&buf);
        let lines: Vec<&str> = text.lines().collect();
        // Unless the window reaches the start of the file, its first line is
        // probably a record the boundary cut in half — drop it.
        let lines = if start > 0 && !lines.is_empty() {
            &lines[1..]
        } else {
            &lines[..]
        };

        let events: Vec<HistoryEvent> = lines
            .iter()
            .rev()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str(l).ok())
            .take(limit)
            .collect();

        // Enough events, or there is no more file to read.
        if events.len() >= limit || start == 0 {
            return Ok(events);
        }
        window *= 2;
    }
}

/// Display history events to stdout.
pub fn display(limit: usize) -> Result<()> {
    let events = read_recent(limit)?;

    if events.is_empty() {
        println!("No workspace history.");
        return Ok(());
    }

    for event in &events {
        let ts = event.timestamp.format("%Y-%m-%d %H:%M");
        let workspace = format!("{}/{}", event.project, event.name);

        let detail = match event.event.as_str() {
            "started" => {
                if let Some(ref issue) = event.from_issue {
                    format!(" (issue {issue})")
                } else {
                    String::new()
                }
            }
            "finished" => {
                let commits = event.commits.unwrap_or(0);
                let strategy = event.merge_strategy.as_deref().unwrap_or("unknown");
                let s = if commits == 1 { "" } else { "s" };
                format!(" ({commits} commit{s}, {strategy})")
            }
            "discarded" => {
                let commits = event.commits.unwrap_or(0);
                if let Some(ref archived) = event.archived_as {
                    format!(" ({commits} commits, archived as {archived})")
                } else {
                    let s = if commits == 1 { "" } else { "s" };
                    format!(" ({commits} commit{s})")
                }
            }
            "restored" => {
                if let Some(ref from) = event.from_branch {
                    format!(" (from {from})")
                } else {
                    String::new()
                }
            }
            "pr_created" => {
                if let Some(ref url) = event.pr_url {
                    format!(" (PR {url})")
                } else {
                    String::new()
                }
            }
            "pr_merged" => {
                if let Some(pr) = event.pr_number {
                    format!(" (PR #{pr})")
                } else {
                    String::new()
                }
            }
            _ => String::new(),
        };

        let (color, label) = match event.event.as_str() {
            "started" => ("\x1b[32m", "started   "),
            "finished" => ("\x1b[34m", "finished  "),
            "discarded" => ("\x1b[33m", "discarded "),
            "restored" => ("\x1b[36m", "restored  "),
            "pr_created" => ("\x1b[35m", "pr        "),
            "pr_merged" => ("\x1b[34m", "merged    "),
            _ => ("", &*format!("{:<10}", event.event)),
        };

        println!("  {ts}  {color}{label}\x1b[0m {workspace}{detail}");
    }

    Ok(())
}
