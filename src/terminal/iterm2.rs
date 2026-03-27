use anyhow::{Context, Result};
use std::path::Path;

use super::applescript::{escape_applescript, pane_var, run_applescript};
use super::{PaneSpec, TerminalBackend};
use crate::config::types::SplitDirection;

pub struct Iterm2Backend;

impl Iterm2Backend {
    /// Detect if we're running inside iTerm2.
    pub fn detect() -> Option<Self> {
        let term = std::env::var("TERM_PROGRAM").ok()?;
        if term.eq_ignore_ascii_case("iterm.app") || term.eq_ignore_ascii_case("iterm2") {
            Some(Self)
        } else {
            None
        }
    }

    /// Build the AppleScript that creates a tab, splits panes, and runs commands.
    fn build_layout_script(path: &Path, panes: &[PaneSpec]) -> Result<String> {
        let path_str = path.to_str().context("invalid worktree path")?;
        let escaped_path = escape_applescript(path_str);
        let mut lines = vec![
            r#"tell application "iTerm2""#.to_string(),
            "    tell current window".to_string(),
            "        create tab with default profile".to_string(),
            "    end tell".to_string(),
        ];

        // Capture a reference to the newly created tab
        lines.push("    set newTab to current tab of current window".to_string());

        if panes.is_empty() {
            // No panes — just cd to the directory and return the session's unique ID
            lines.push("    set firstSession to current session of newTab".to_string());
            lines.push("    tell firstSession".to_string());
            lines.push(format!("        write text \"cd {escaped_path}\""));
            lines.push("    end tell".to_string());
            lines.push("    return unique id of firstSession".to_string());
            lines.push("end tell".to_string());
            return Ok(lines.join("\n"));
        }

        // Get the first session (the one created with the tab)
        let first = &panes[0];
        let first_var = pane_var(&first.name);
        lines.push(format!("    set {first_var} to current session of newTab"));

        // cd the first pane to the worktree
        lines.push(format!("    tell {first_var}"));
        lines.push(format!("        write text \"cd {escaped_path}\""));
        lines.push("    end tell".to_string());

        // Create splits for remaining panes
        for pane in &panes[1..] {
            let var = pane_var(&pane.name);
            let split_from = pane
                .split_from
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("pane '{}' has no split_from", pane.name))?;
            let parent_var = pane_var(split_from);

            // iTerm2 split naming: "vertically" creates a vertical divider (left/right panes),
            // "horizontally" creates a horizontal divider (top/bottom panes).
            let split_dir = match pane
                .direction
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("pane '{}' has no direction", pane.name))?
            {
                SplitDirection::Right => "vertically",
                SplitDirection::Down => "horizontally",
            };

            lines.push(format!("    tell {parent_var}"));
            lines.push(format!(
                "        set {var} to split {split_dir} with default profile"
            ));
            lines.push("    end tell".to_string());

            // cd the new pane to the worktree
            lines.push(format!("    tell {var}"));
            lines.push(format!("        write text \"cd {escaped_path}\""));
            lines.push("    end tell".to_string());
        }

        // Export env vars and run commands in each pane
        for pane in panes {
            let var = pane_var(&pane.name);

            // Export environment variables
            if !pane.env.is_empty() {
                lines.push(format!("    tell {var}"));
                for (k, v) in &pane.env {
                    let escaped_k = escape_applescript(k);
                    let escaped_v = escape_applescript(v);
                    lines.push(format!(
                        "        write text \"export {escaped_k}='{escaped_v}'\""
                    ));
                }
                lines.push("    end tell".to_string());
            }

            // Run the pane command
            if let Some(ref cmd) = pane.command
                && !cmd.is_empty()
            {
                let escaped_cmd = escape_applescript(cmd);
                lines.push(format!("    tell {var}"));
                lines.push(format!("        write text \"{escaped_cmd}\""));
                lines.push("    end tell".to_string());
            }
        }

        // Focus the first pane
        lines.push(format!("    select {first_var}"));

        // Return the first session's unique ID for later tab lookup
        lines.push(format!("    return unique id of {first_var}"));
        lines.push("end tell".to_string());

        Ok(lines.join("\n"))
    }
}

impl TerminalBackend for Iterm2Backend {
    fn open_workspace(&self, path: &Path, panes: &[PaneSpec], verbose: bool) -> Result<String> {
        if verbose {
            eprintln!("Opening new iTerm2 tab...");
        }

        let script = Self::build_layout_script(path, panes)?;

        if verbose {
            eprintln!("Setting up workspace layout...");
        }

        let tab_id = run_applescript(&script)?;

        // Return the iTerm2 tab ID for later reference.
        Ok(tab_id)
    }

    fn close_tab(&self, tab_id: &str) -> Result<()> {
        if tab_id.is_empty() {
            return Ok(());
        }

        let escaped_id = escape_applescript(tab_id);
        let script = format!(
            r#"tell application "iTerm2"
    try
        repeat with w in windows
            repeat with t in tabs of w
                repeat with s in sessions of t
                    if unique id of s is "{escaped_id}" then
                        close t
                        return
                    end if
                end repeat
            end repeat
        end repeat
    end try
end tell"#
        );

        let _ = run_applescript(&script);
        Ok(())
    }

    fn run_in_pane(&self, tab_id: &str, pane_index: usize, command: &str) -> Result<()> {
        if tab_id.is_empty() {
            return Ok(());
        }

        let escaped_id = escape_applescript(tab_id);
        let escaped_cmd = escape_applescript(command);
        // AppleScript uses 1-based indexing
        let as_index = pane_index + 1;

        let script = format!(
            r#"tell application "iTerm2"
    repeat with w in windows
        repeat with t in tabs of w
            repeat with s in sessions of t
                if unique id of s is "{escaped_id}" then
                    set targetSession to item {as_index} of sessions of t
                    tell targetSession
                        write text "{escaped_cmd}"
                    end tell
                    return
                end if
            end repeat
        end repeat
    end repeat
end tell"#
        );

        run_applescript(&script)?;
        Ok(())
    }

    fn focus_tab(&self, tab_id: &str) -> Result<bool> {
        if tab_id.is_empty() {
            return Ok(false);
        }

        let escaped_id = escape_applescript(tab_id);
        let script = format!(
            r#"tell application "iTerm2"
    activate
    repeat with w in windows
        repeat with t in tabs of w
            repeat with s in sessions of t
                if unique id of s is "{escaped_id}" then
                    select t
                    return "found"
                end if
            end repeat
        end repeat
    end repeat
    return "not_found"
end tell"#
        );

        let result = run_applescript(&script)?;
        Ok(result.contains("found") && !result.contains("not_found"))
    }
}
