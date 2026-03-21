use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use crate::config::types::SplitDirection;
use super::{PaneSpec, TerminalBackend};

pub struct GhosttyBackend;

impl GhosttyBackend {
    /// Detect if we're running inside Ghostty.
    pub fn detect() -> Option<Self> {
        let term = std::env::var("TERM_PROGRAM").ok()?;
        if term.eq_ignore_ascii_case("ghostty") {
            Some(Self)
        } else {
            None
        }
    }

    fn run_applescript(script: &str) -> Result<String> {
        let output = Command::new("osascript")
            .arg("-e")
            .arg(script)
            .output()
            .context("failed to run osascript")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("AppleScript error: {}", stderr.trim());
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Escape a string for use inside AppleScript double-quoted strings.
    fn escape_applescript(s: &str) -> String {
        s.replace('\\', "\\\\").replace('"', "\\\"")
    }

    /// Build a variable name for a pane (sanitize the pane name for AppleScript).
    fn pane_var(name: &str) -> String {
        // Replace non-alphanumeric chars with underscores for valid AppleScript variable names
        let sanitized: String = name
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect();
        format!("pane_{sanitized}")
    }

    /// Build the complete AppleScript to create a workspace layout.
    fn build_workspace_script(
        path: &Path,
        panes: &[PaneSpec],
    ) -> Result<String> {
        let path_str = path.to_str().context("invalid worktree path")?;
        let mut lines = Vec::new();

        lines.push(r#"tell application "Ghostty""#.to_string());
        lines.push("    activate".to_string());

        // Create the base configuration with working directory
        lines.push("    set cfg to new surface configuration".to_string());
        lines.push(format!(
            "    set initial working directory of cfg to \"{}\"",
            Self::escape_applescript(path_str)
        ));

        if panes.is_empty() {
            // No pane config — just open a new tab
            lines.push("    new tab with configuration cfg".to_string());
            lines.push("end tell".to_string());
            return Ok(lines.join("\n"));
        }

        // First pane: create a new tab (defaults to front window)
        let first = &panes[0];
        let first_var = Self::pane_var(&first.name);

        // If the first pane has env vars, set them on the config
        if !first.env.is_empty() {
            let env_list = Self::build_env_list(&first.env);
            lines.push(format!(
                "    set environment variables of cfg to {env_list}"
            ));
        }

        lines.push("    set newTab to new tab with configuration cfg".to_string());
        lines.push(format!(
            "    set {first_var} to terminal 1 of newTab"
        ));

        // Process remaining panes
        for pane in &panes[1..] {
            let pane_var = Self::pane_var(&pane.name);
            let split_from = pane.split_from.as_ref()
                .ok_or_else(|| anyhow::anyhow!("pane '{}' has no split_from", pane.name))?;
            let parent_var = Self::pane_var(split_from);

            let dir_str = match pane.direction.as_ref()
                .ok_or_else(|| anyhow::anyhow!("pane '{}' has no direction", pane.name))?
            {
                SplitDirection::Right => "right",
                SplitDirection::Down => "down",
            };

            // If this pane has different env vars, create a new config
            if !pane.env.is_empty() {
                let pane_cfg_var = format!("cfg_{}", Self::pane_var(&pane.name));
                lines.push(format!("    set {pane_cfg_var} to new surface configuration"));
                lines.push(format!(
                    "    set initial working directory of {pane_cfg_var} to \"{}\"",
                    Self::escape_applescript(path_str)
                ));
                let env_list = Self::build_env_list(&pane.env);
                lines.push(format!(
                    "    set environment variables of {pane_cfg_var} to {env_list}"
                ));
                lines.push(format!(
                    "    set {pane_var} to split {parent_var} direction {dir_str} with configuration {pane_cfg_var}"
                ));
            } else {
                lines.push(format!(
                    "    set {pane_var} to split {parent_var} direction {dir_str} with configuration cfg"
                ));
            }
        }

        // Now send commands to each pane
        for pane in panes {
            if let Some(ref cmd) = pane.command {
                if !cmd.is_empty() {
                    let pane_var = Self::pane_var(&pane.name);
                    let escaped_cmd = Self::escape_applescript(cmd);
                    lines.push(format!(
                        "    input text \"{escaped_cmd}\" to {pane_var}"
                    ));
                    lines.push(format!(
                        "    send key \"enter\" to {pane_var}"
                    ));
                }
            }
        }

        // Focus the first pane
        lines.push(format!("    focus {first_var}"));

        lines.push("end tell".to_string());

        Ok(lines.join("\n"))
    }

    /// Build an AppleScript list literal for environment variables.
    /// Example: {"KEY1=value1", "KEY2=value2"}
    fn build_env_list(env: &HashMap<String, String>) -> String {
        let items: Vec<String> = env
            .iter()
            .map(|(k, v)| format!("\"{}={}\"", Self::escape_applescript(k), Self::escape_applescript(v)))
            .collect();
        format!("{{{}}}", items.join(", "))
    }
}

impl TerminalBackend for GhosttyBackend {
    fn open_workspace(&self, path: &Path, panes: &[PaneSpec], verbose: bool) -> Result<String> {
        let script = Self::build_workspace_script(path, panes)?;

        if verbose {
            eprintln!("Running Ghostty AppleScript...");
        }

        Self::run_applescript(&script)?;

        // Return the worktree path as the tab identifier.
        // For close_tab, we'll find the tab by matching terminal working directories.
        Ok(path.to_string_lossy().into())
    }

    fn close_tab(&self, tab_id: &str) -> Result<()> {
        if tab_id.is_empty() {
            return Ok(());
        }

        // Find and close the tab containing a terminal whose working directory
        // matches our worktree path. This works across CLI invocations since
        // we stored the worktree path as the tab_id.
        let escaped_path = Self::escape_applescript(tab_id);
        let script = format!(
            r#"tell application "Ghostty"
    try
        set targetTerms to every terminal whose working directory contains "{escaped_path}"
        if (count of targetTerms) > 0 then
            set targetTerm to item 1 of targetTerms
            close targetTerm
        end if
    end try
end tell"#
        );

        // Ignore errors — tab may already be closed
        let _ = Self::run_applescript(&script);
        Ok(())
    }
}
