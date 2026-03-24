use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use super::{PaneSpec, TerminalBackend};
use crate::config::types::SplitDirection;

pub struct ZellijBackend;

impl ZellijBackend {
    /// Detect if Zellij is available on the system.
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
    ///
    /// Converts foundry's `split_from` + `direction` tree into Zellij's nested
    /// container model. Each split creates a directional container:
    /// - `SplitDirection::Right` → `direction="vertical"` (side-by-side)
    /// - `SplitDirection::Down` → `direction="horizontal"` (stacked)
    fn build_layout(path: &Path, panes: &[PaneSpec]) -> Result<String> {
        let path_str = path.to_str().context("invalid worktree path")?;

        if panes.is_empty() {
            return Ok(format!("layout {{\n    cwd \"{path_str}\"\n    pane\n}}\n"));
        }

        let escaped_cwd = path_str.replace('"', "\\\"");

        let mut lines = Vec::new();
        lines.push("layout {".into());
        lines.push(format!("    cwd \"{escaped_cwd}\""));

        // Start from the first pane (root of the split tree)
        if let Some(root) = panes.first() {
            let pane_lines = Self::render_pane(root, panes, 4);
            lines.extend(pane_lines);
        }

        lines.push("}".into());
        Ok(lines.join("\n"))
    }

    /// Recursively render a pane and its children (panes that split from it).
    fn render_pane(pane: &PaneSpec, all_panes: &[PaneSpec], indent: usize) -> Vec<String> {
        // Find children that split from this pane
        let children: Vec<&PaneSpec> = all_panes
            .iter()
            .filter(|p| p.split_from.as_deref() == Some(&pane.name))
            .collect();

        if children.is_empty() {
            // Leaf pane — no children split from it
            return Self::render_pane_node(pane, indent);
        }

        // This pane has children. For each child, wrap the current pane and
        // the child in a container with the child's split direction.
        let pad = " ".repeat(indent);
        let mut current = Self::render_pane_node(pane, indent + 4);

        for child in &children {
            let dir_str = match child.direction {
                Some(SplitDirection::Right) => "Vertical",
                Some(SplitDirection::Down) => "Horizontal",
                None => "Vertical",
            };

            let child_lines = Self::render_pane(child, all_panes, indent + 4);

            let mut wrapped = Vec::new();
            wrapped.push(format!("{pad}pane split_direction=\"{dir_str}\" {{"));
            wrapped.extend(current);
            wrapped.extend(child_lines);
            wrapped.push(format!("{pad}}}"));
            current = wrapped;
        }

        current
    }

    /// Render a single pane node (leaf, no container wrapping).
    fn render_pane_node(pane: &PaneSpec, indent: usize) -> Vec<String> {
        let pad = " ".repeat(indent);

        if let Some(ref cmd) = pane.command {
            if !cmd.is_empty() {
                let mut full_cmd = String::new();
                for (k, v) in &pane.env {
                    full_cmd.push_str(&format!("export {k}='{}'; ", v.replace('\'', "'\\''")));
                }
                full_cmd.push_str(cmd);
                let escaped = full_cmd.replace('"', "\\\"");
                return vec![
                    format!("{pad}pane command=\"bash\" name=\"{}\" {{", pane.name),
                    format!("{pad}    args \"-c\" \"{escaped}\""),
                    format!("{pad}}}"),
                ];
            }
        }

        vec![format!("{pad}pane name=\"{}\"", pane.name)]
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
    fn supports_run_in_pane(&self) -> bool {
        false
    }

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
            eprintln!("Layout file: {}", layout_path.display());
            eprintln!("{layout}");
        }

        if verbose {
            eprintln!(
                "Setting up workspace layout with {} pane(s)...",
                panes.len()
            );
        }

        // Clean up any dead session with the same name from a previous run
        let _ = Command::new("zellij")
            .args(["delete-session", &session])
            .output();

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

        let _ = child.wait();

        // Clean up layout file (keep if verbose for debugging)
        if !verbose {
            let _ = std::fs::remove_file(&layout_path);
        }

        Ok(session)
    }

    fn close_tab(&self, tab_id: &str) -> Result<()> {
        if tab_id.is_empty() {
            return Ok(());
        }

        if Self::inside_zellij() {
            // Inside the session — close the current tab. If it's the only
            // tab, this exits the session.
            let _ = Command::new("zellij").args(["action", "close-tab"]).spawn();
        } else {
            // Outside the session — kill it by name
            let _ = Command::new("zellij")
                .args(["kill-session", tab_id])
                .spawn();
        }

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
