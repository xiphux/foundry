use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use super::{PaneSpec, TerminalBackend};
use crate::config::types::SplitDirection;

pub struct TmuxBackend;

impl TmuxBackend {
    /// Detect if tmux is available on the system.
    /// Only used as a fallback when no native terminal backend is detected.
    pub fn detect() -> Option<Self> {
        Command::new("tmux")
            .arg("-V")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|_| Self)
    }

    /// Check if we're currently inside a tmux session.
    /// Used to prevent nesting (opening a new session inside an existing one).
    pub fn inside_tmux() -> bool {
        std::env::var("TMUX").is_ok()
    }

    /// Generate a session name from the worktree path.
    fn session_name(path: &Path) -> String {
        let name = path
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("workspace");
        let parent = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|f| f.to_str())
            .unwrap_or("foundry");
        // tmux session names can't contain dots or colons
        format!("foundry-{parent}-{name}")
            .replace('.', "-")
            .replace(':', "-")
    }

    /// Run a tmux command and return stdout.
    fn run_tmux(args: &[&str]) -> Result<String> {
        let output = Command::new("tmux")
            .args(args)
            .output()
            .context("failed to run tmux")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("tmux error: {}", stderr.trim());
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

impl TerminalBackend for TmuxBackend {
    fn supports_run_in_pane(&self) -> bool {
        false
    }

    fn open_workspace(&self, path: &Path, panes: &[PaneSpec], verbose: bool) -> Result<String> {
        if Self::inside_tmux() {
            anyhow::bail!(
                "already inside a tmux session. Cannot open a nested workspace. \
                 Detach first (Ctrl+B, D) and run foundry from outside tmux."
            );
        }

        let session = Self::session_name(path);
        let path_str = path.to_str().context("invalid worktree path")?;
        let user_shell = std::env::var("SHELL").unwrap_or_else(|_| "bash".into());

        if verbose {
            eprintln!("Starting tmux session '{session}'...");
        }

        // Create a new detached session with the first pane
        let first_cmd = panes
            .first()
            .and_then(|p| p.command.as_deref())
            .unwrap_or(&user_shell);

        // Build env exports for the first pane
        let first_env = panes
            .first()
            .map(|p| {
                p.env
                    .iter()
                    .map(|(k, v)| format!("export {k}='{}'", v.replace('\'', "'\\''")))
                    .collect::<Vec<_>>()
                    .join("; ")
            })
            .unwrap_or_default();

        let first_full_cmd = if first_env.is_empty() {
            first_cmd.to_string()
        } else {
            format!("{first_env}; {first_cmd}")
        };

        Self::run_tmux(&[
            "new-session",
            "-d",
            "-s",
            &session,
            "-c",
            path_str,
            &user_shell,
            "-c",
            &first_full_cmd,
        ])?;

        if verbose {
            eprintln!(
                "Setting up workspace layout with {} pane(s)...",
                panes.len()
            );
        }

        // Create splits for remaining panes.
        // tmux pane IDs (%N) are globally unique — use them directly as targets.
        let mut pane_ids: Vec<String> = Vec::new();

        let first_pane_id = Self::run_tmux(&["list-panes", "-t", &session, "-F", "#{pane_id}"])?;
        pane_ids.push(first_pane_id.lines().next().unwrap_or("%0").to_string());

        for pane in panes.iter().skip(1) {
            let split_from = pane
                .split_from
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("pane '{}' has no split_from", pane.name))?;

            // Find the tmux pane ID for the parent
            let parent_idx = panes
                .iter()
                .position(|p| p.name == *split_from)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "pane '{}' references unknown split_from '{}'",
                        pane.name,
                        split_from
                    )
                })?;
            let parent_pane_id = &pane_ids[parent_idx];

            let split_flag = match pane
                .direction
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("pane '{}' has no direction", pane.name))?
            {
                SplitDirection::Right => "-h",
                SplitDirection::Down => "-v",
            };

            // Build command with env vars
            let pane_cmd = pane.command.as_deref().unwrap_or(&user_shell);
            let env_exports: String = pane
                .env
                .iter()
                .map(|(k, v)| format!("export {k}='{}'", v.replace('\'', "'\\''")))
                .collect::<Vec<_>>()
                .join("; ");

            let full_cmd = if env_exports.is_empty() {
                pane_cmd.to_string()
            } else {
                format!("{env_exports}; {pane_cmd}")
            };

            let new_pane_id = Self::run_tmux(&[
                "split-window",
                split_flag,
                "-t",
                parent_pane_id,
                "-c",
                path_str,
                "-P",
                "-F",
                "#{pane_id}",
                &user_shell,
                "-c",
                &full_cmd,
            ])?;

            pane_ids.push(new_pane_id.trim().to_string());
        }

        // Select the first pane
        if let Some(first) = pane_ids.first() {
            let _ = Self::run_tmux(&["select-pane", "-t", first]);
        }

        // Attach to the session (takes over the current terminal)
        let mut child = Command::new("tmux")
            .args(["attach-session", "-t", &session])
            .spawn()
            .context("failed to attach to tmux session")?;
        let _ = child.wait();

        Ok(session)
    }

    fn close_tab(&self, tab_id: &str) -> Result<()> {
        if tab_id.is_empty() {
            return Ok(());
        }

        // Use spawn instead of output — when killing our own session from
        // inside it, the process may be terminated before output() returns.
        // Spawning fire-and-forget avoids hanging.
        let _ = Command::new("tmux")
            .args(["kill-session", "-t", tab_id])
            .spawn();
        Ok(())
    }

    fn focus_tab(&self, tab_id: &str) -> Result<bool> {
        if tab_id.is_empty() {
            return Ok(false);
        }

        // Check if session exists
        let result = Self::run_tmux(&["has-session", "-t", tab_id]);
        if result.is_err() {
            return Ok(false);
        }

        // Attach to the session
        let mut child = Command::new("tmux")
            .args(["attach-session", "-t", tab_id])
            .spawn()
            .context("failed to attach to tmux session")?;
        let _ = child.wait();

        Ok(true)
    }

    fn run_in_pane(&self, tab_id: &str, pane_index: usize, command: &str) -> Result<()> {
        if tab_id.is_empty() {
            return Ok(());
        }

        // Get pane IDs for this session. If the server is no longer running
        // (session already exited), silently skip.
        let output = match Self::run_tmux(&["list-panes", "-t", tab_id, "-F", "#{pane_id}"]) {
            Ok(o) => o,
            Err(e) if e.to_string().contains("no server running") => return Ok(()),
            Err(e) => return Err(e),
        };
        let pane_ids: Vec<&str> = output.lines().collect();

        let pane_id = pane_ids.get(pane_index).ok_or_else(|| {
            anyhow::anyhow!(
                "pane index {pane_index} out of range (session has {} panes)",
                pane_ids.len()
            )
        })?;

        // Send keys to the target pane
        Self::run_tmux(&["send-keys", "-t", pane_id, command, "Enter"])?;

        Ok(())
    }
}
