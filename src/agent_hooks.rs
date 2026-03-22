use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::config;

/// The possible agent statuses we track.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AgentStatus {
    Working,
    Idle,
    WaitingPermission,
    Unknown,
}

impl AgentStatus {
    pub fn label(self) -> &'static str {
        match self {
            AgentStatus::Working => "working",
            AgentStatus::Idle => "idle",
            AgentStatus::WaitingPermission => "waiting for permission",
            AgentStatus::Unknown => "unknown",
        }
    }
}

/// Get the path to the status file for a workspace.
pub fn status_file_path(project: &str, name: &str) -> Result<PathBuf> {
    let foundry_dir = config::foundry_dir()?;
    Ok(foundry_dir
        .join("status")
        .join(project)
        .join(format!("{name}.status")))
}

/// Read the current agent status from the status file.
pub fn read_status(project: &str, name: &str) -> AgentStatus {
    let path = match status_file_path(project, name) {
        Ok(p) => p,
        Err(_) => return AgentStatus::Unknown,
    };

    match std::fs::read_to_string(&path) {
        Ok(content) => match content.trim() {
            "working" => AgentStatus::Working,
            "idle" => AgentStatus::Idle,
            "waiting_permission" => AgentStatus::WaitingPermission,
            _ => AgentStatus::Unknown,
        },
        Err(_) => AgentStatus::Unknown,
    }
}

/// Remove the status file for a workspace (cleanup on finish/discard).
pub fn remove_status(project: &str, name: &str) {
    if let Ok(path) = status_file_path(project, name) {
        let _ = std::fs::remove_file(&path);
    }
}

/// Create the .claude/settings.local.json in the worktree with hooks
/// that write agent status to the foundry status file.
pub fn setup_agent_hooks(worktree_path: &Path, project: &str, name: &str) -> Result<()> {
    let status_path = status_file_path(project, name)?;
    let status_path_str = status_path.to_string_lossy();

    // Ensure the status directory exists
    if let Some(parent) = status_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create status directory {}", parent.display()))?;
    }

    // Write initial status
    std::fs::write(&status_path, "idle")
        .with_context(|| format!("failed to write status file {}", status_path.display()))?;

    let claude_dir = worktree_path.join(".claude");
    std::fs::create_dir_all(&claude_dir).with_context(|| {
        format!(
            "failed to create .claude directory in {}",
            worktree_path.display()
        )
    })?;

    let settings_path = claude_dir.join("settings.local.json");

    // Build the hook script commands. Each writes a status string to the file.
    // We use simple shell commands via the "command" hook type.
    let settings = serde_json::json!({
        "hooks": {
            "UserPromptSubmit": [
                {
                    "matcher": "*",
                    "hooks": [
                        {
                            "type": "command",
                            "command": format!("echo working > '{status_path_str}'"),
                            "async": true
                        }
                    ]
                }
            ],
            "Stop": [
                {
                    "matcher": "*",
                    "hooks": [
                        {
                            "type": "command",
                            "command": format!("echo idle > '{status_path_str}'"),
                            "async": true
                        }
                    ]
                }
            ],
            "Notification": [
                {
                    "matcher": "permission_prompt",
                    "hooks": [
                        {
                            "type": "command",
                            "command": format!("echo waiting_permission > '{status_path_str}'"),
                            "async": true
                        }
                    ]
                }
            ]
        }
    });

    let contents =
        serde_json::to_string_pretty(&settings).context("failed to serialize hook settings")?;

    std::fs::write(&settings_path, contents)
        .with_context(|| format!("failed to write {}", settings_path.display()))?;

    Ok(())
}
