use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};

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

/// Read the most recent history events, up to `limit`.
pub fn read_recent(limit: usize) -> Result<Vec<HistoryEvent>> {
    let path = history_path()?;

    if !path.exists() {
        return Ok(Vec::new());
    }

    let file = File::open(&path)
        .with_context(|| format!("failed to open history log at {}", path.display()))?;

    let reader = BufReader::new(file);
    let mut events: Vec<HistoryEvent> = reader
        .lines()
        .filter_map(|line| {
            let line = line.ok()?;
            if line.trim().is_empty() {
                return None;
            }
            serde_json::from_str(&line).ok()
        })
        .collect();

    // Return most recent first
    events.reverse();
    events.truncate(limit);
    Ok(events)
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
