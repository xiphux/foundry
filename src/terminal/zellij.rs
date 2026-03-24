use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use super::{PaneSpec, TerminalBackend};

pub struct ZellijBackend;

impl ZellijBackend {
    /// Detect if Zellij is available on the system.
    /// Only used as a fallback when no native terminal backend is detected.
    pub fn detect() -> Option<Self> {
        Command::new("zellij")
            .arg("--version")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|_| Self)
    }

    /// Check if we're currently inside a Zellij session.
    pub fn inside_zellij() -> bool {
        std::env::var("ZELLIJ").is_ok()
    }

    /// Build a Zellij KDL layout string from pane specs.
    fn build_layout(path: &Path, panes: &[PaneSpec]) -> Result<String> {
        let path_str = path.to_str().context("invalid worktree path")?;

        if panes.is_empty() {
            return Ok(format!("layout {{\n    cwd \"{path_str}\"\n    pane\n}}\n"));
        }

        // Zellij layouts use a tree structure of panes with horizontal/vertical splits.
        // For simplicity, we build a flat layout: all panes in a single horizontal
        // container, splitting vertically (side-by-side) or nested.
        //
        // Since foundry's split model is tree-based (split_from + direction), and Zellij's
        // layout model is nested containers, we simplify: generate panes in order with
        // their commands. Complex split topologies may not match exactly.
        let mut lines = Vec::new();
        lines.push("layout {".into());
        lines.push(format!("    cwd \"{}\"", path_str.replace('"', "\\\"")));

        // Build a simple layout: first pane, then splits
        // We use a flat pane list — Zellij will arrange them in its default layout
        for pane in panes {
            let mut pane_line = String::from("    pane");
            if let Some(ref cmd) = pane.command {
                if !cmd.is_empty() {
                    // Build command with env vars prepended
                    let mut full_cmd = String::new();
                    for (k, v) in &pane.env {
                        full_cmd.push_str(&format!("export {k}='{}'; ", v.replace('\'', "'\\''")));
                    }
                    full_cmd.push_str(cmd);
                    pane_line.push_str(&format!(
                        " command=\"bash\" {{ args \"-c\" \"{}\" }}",
                        full_cmd.replace('"', "\\\"")
                    ));
                }
            }
            pane_line.push_str(&format!(" {{ name \"{}\" }}", pane.name));
            lines.push(pane_line);
        }

        lines.push("}".into());
        Ok(lines.join("\n"))
    }

    /// Generate a session name from project/workspace info.
    /// Zellij has a ~25 character limit on session names, so we use a
    /// short hash to keep names unique within the limit.
    fn session_name(path: &Path) -> String {
        use std::hash::{Hash, Hasher};
        let name = path.file_name().and_then(|f| f.to_str()).unwrap_or("ws");
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        path.hash(&mut hasher);
        let hash = format!("{:x}", hasher.finish());
        let hash_short = &hash[..6];
        // Truncate workspace name to fit: "f-" (2) + name + "-" (1) + hash (6) <= 25
        let max_name_len = 16;
        let truncated = if name.len() > max_name_len {
            &name[..max_name_len]
        } else {
            name
        };
        format!("f-{truncated}-{hash_short}")
    }
}

impl TerminalBackend for ZellijBackend {
    fn open_workspace(&self, path: &Path, panes: &[PaneSpec], verbose: bool) -> Result<String> {
        if Self::inside_zellij() {
            anyhow::bail!(
                "already inside a Zellij session. Cannot open a nested workspace. \
                 Detach first (Ctrl+O, D) and run foundry from outside Zellij."
            );
        }

        let session = Self::session_name(path);

        if verbose {
            eprintln!("Starting Zellij session '{session}'...");
        }

        // Write a temp layout file
        let layout = Self::build_layout(path, panes)?;
        let layout_dir = std::env::temp_dir().join("foundry-zellij-layouts");
        std::fs::create_dir_all(&layout_dir)?;
        let layout_path = layout_dir.join(format!("{session}.kdl"));
        std::fs::write(&layout_path, &layout)?;

        if verbose {
            eprintln!(
                "Setting up workspace layout with {} pane(s)...",
                panes.len()
            );
        }

        // Start Zellij in the background — it takes over a new process
        // We use `spawn` rather than `exec` so foundry can continue to
        // record state after the session starts.
        let mut child = Command::new("zellij")
            .args([
                "--session",
                &session,
                "--new-session-with-layout",
                layout_path.to_str().context("invalid layout path")?,
            ])
            .current_dir(path)
            .spawn()
            .context("failed to start zellij")?;

        // Detach immediately so foundry can continue.
        // Zellij runs in the foreground of its own terminal — we don't wait.
        // However, if we're in a pipe/non-interactive context, we should wait.
        // For now, just let it run.
        let _ = child.wait();

        // Clean up layout file
        let _ = std::fs::remove_file(&layout_path);

        Ok(session)
    }

    fn close_tab(&self, tab_id: &str) -> Result<()> {
        if tab_id.is_empty() {
            return Ok(());
        }

        let _ = Command::new("zellij")
            .args(["kill-session", tab_id])
            .output();

        Ok(())
    }

    fn focus_tab(&self, tab_id: &str) -> Result<bool> {
        if tab_id.is_empty() {
            return Ok(false);
        }

        // Check if session exists
        let output = Command::new("zellij")
            .args(["list-sessions"])
            .output()
            .context("failed to list zellij sessions")?;

        let sessions = String::from_utf8_lossy(&output.stdout);
        if !sessions.lines().any(|l| l.contains(tab_id)) {
            return Ok(false);
        }

        // Attach to the session (takes over the current terminal)
        let mut child = Command::new("zellij")
            .args(["attach", tab_id])
            .spawn()
            .context("failed to attach to zellij session")?;
        let _ = child.wait();

        Ok(true)
    }

    fn run_in_pane(&self, tab_id: &str, _pane_index: usize, command: &str) -> Result<()> {
        if tab_id.is_empty() {
            return Ok(());
        }

        // Zellij's CLI can write to panes, but targeting by index requires
        // the session to be running and us to be attached. For now, use
        // zellij action write-chars which sends to the focused pane.
        let cmd_with_enter = format!("{command}\n");
        let _ = Command::new("zellij")
            .args([
                "--session",
                tab_id,
                "action",
                "write-chars",
                &cmd_with_enter,
            ])
            .output();

        Ok(())
    }
}
