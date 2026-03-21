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

    /// Run an AppleScript, ignoring errors. Used for commands like `new tab`
    /// which succeed but throw a spurious error in Ghostty 1.x.
    fn run_applescript_ignoring_errors(script: &str) -> Result<()> {
        let _ = Command::new("osascript")
            .arg("-e")
            .arg(script)
            .output()
            .context("failed to run osascript")?;
        Ok(())
    }

    /// Escape a string for use inside AppleScript double-quoted strings.
    fn escape_applescript(s: &str) -> String {
        s.replace('\\', "\\\\").replace('"', "\\\"")
    }

    /// Build a variable name for a pane (sanitize the pane name for AppleScript).
    fn pane_var(name: &str) -> String {
        let sanitized: String = name
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect();
        format!("pane_{sanitized}")
    }

    /// Build the main AppleScript that sets up splits and runs commands.
    /// This runs AFTER the tab has already been created via a separate call.
    fn build_layout_script(
        path: &Path,
        panes: &[PaneSpec],
    ) -> Result<String> {
        let path_str = path.to_str().context("invalid worktree path")?;
        let mut lines = Vec::new();

        lines.push(r#"tell application "Ghostty""#.to_string());

        if panes.is_empty() {
            // No panes to configure — just cd to the directory
            lines.push(
                "    set t to focused terminal of selected tab of front window".to_string(),
            );
            let escaped_path = Self::escape_applescript(path_str);
            lines.push(format!(
                "    input text \"cd {escaped_path}\" to t"
            ));
            lines.push("    send key \"enter\" to t".to_string());
            lines.push("end tell".to_string());
            return Ok(lines.join("\n"));
        }

        // Create the base configuration with working directory (used for splits)
        lines.push("    set cfg to new surface configuration".to_string());
        lines.push(format!(
            "    set initial working directory of cfg to \"{}\"",
            Self::escape_applescript(path_str)
        ));

        // Get the first pane — it's the terminal in the tab we just created
        let first = &panes[0];
        let first_var = Self::pane_var(&first.name);
        lines.push(format!(
            "    set {first_var} to focused terminal of selected tab of front window"
        ));

        // cd the first pane to the worktree (since new tab didn't get a configuration)
        let escaped_path = Self::escape_applescript(path_str);
        lines.push(format!(
            "    input text \"cd {escaped_path}\" to {first_var}"
        ));
        lines.push(format!("    send key \"enter\" to {first_var}"));

        // Process remaining panes — create splits
        for pane in &panes[1..] {
            let pane_var = Self::pane_var(&pane.name);
            let split_from = pane
                .split_from
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("pane '{}' has no split_from", pane.name))?;
            let parent_var = Self::pane_var(split_from);

            let dir_str = match pane
                .direction
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("pane '{}' has no direction", pane.name))?
            {
                SplitDirection::Right => "right",
                SplitDirection::Down => "down",
            };

            // If this pane has env vars, create a custom config
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

        // Send commands to each pane
        for pane in panes {
            if let Some(ref cmd) = pane.command {
                if !cmd.is_empty() {
                    let pane_var = Self::pane_var(&pane.name);
                    // If pane has env vars and it's the first pane (which didn't get
                    // a surface configuration), export them manually
                    if pane.split_from.is_none() && !pane.env.is_empty() {
                        for (k, v) in &pane.env {
                            let escaped_k = Self::escape_applescript(k);
                            let escaped_v = Self::escape_applescript(v);
                            lines.push(format!(
                                "    input text \"export {escaped_k}='{escaped_v}'\" to {pane_var}"
                            ));
                            lines.push(format!("    send key \"enter\" to {pane_var}"));
                        }
                    }
                    let escaped_cmd = Self::escape_applescript(cmd);
                    lines.push(format!(
                        "    input text \"{escaped_cmd}\" to {pane_var}"
                    ));
                    lines.push(format!("    send key \"enter\" to {pane_var}"));
                }
            }
        }

        // Focus the first pane
        lines.push(format!("    focus {first_var}"));

        lines.push("end tell".to_string());

        Ok(lines.join("\n"))
    }

    /// Build an AppleScript list literal for environment variables.
    fn build_env_list(env: &HashMap<String, String>) -> String {
        let items: Vec<String> = env
            .iter()
            .map(|(k, v)| {
                format!(
                    "\"{}={}\"",
                    Self::escape_applescript(k),
                    Self::escape_applescript(v)
                )
            })
            .collect();
        format!("{{{}}}", items.join(", "))
    }
}

impl TerminalBackend for GhosttyBackend {
    fn open_workspace(&self, path: &Path, panes: &[PaneSpec], verbose: bool) -> Result<String> {
        // Step 1: Create a new tab.
        // Ghostty 1.x has a bug where `new tab` succeeds but throws a spurious error,
        // so we run it separately and ignore the error.
        if verbose {
            eprintln!("Opening new Ghostty tab...");
        }
        Self::run_applescript_ignoring_errors(
            r#"tell application "Ghostty" to new tab"#,
        )?;

        // Brief pause to let the tab finish creating
        std::thread::sleep(std::time::Duration::from_millis(500));

        // Step 2: Build and run the layout script (splits, commands)
        let script = Self::build_layout_script(path, panes)?;

        if verbose {
            eprintln!("Setting up workspace layout...");
        }

        Self::run_applescript(&script)?;

        // Return the worktree path as the tab identifier.
        Ok(path.to_string_lossy().into())
    }

    fn close_tab(&self, tab_id: &str) -> Result<()> {
        if tab_id.is_empty() {
            return Ok(());
        }

        // Iterate through all tabs in all windows, find the tab containing
        // a terminal whose working directory matches the worktree path,
        // and close the entire tab (not just one pane).
        let escaped_path = Self::escape_applescript(tab_id);
        let script = format!(
            r#"tell application "Ghostty"
    try
        repeat with w in windows
            repeat with t in tabs of w
                set terms to terminals of t
                repeat with term in terms
                    if working directory of term contains "{escaped_path}" then
                        close tab t
                        return
                    end if
                end repeat
            end repeat
        end repeat
    end try
end tell"#
        );

        let _ = Self::run_applescript(&script);
        Ok(())
    }
}
