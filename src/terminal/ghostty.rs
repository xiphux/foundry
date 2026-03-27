use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;

use super::applescript::{
    escape_applescript, pane_var, run_applescript, run_applescript_ignoring_errors,
};
use super::{PaneSpec, TerminalBackend};
use crate::config::types::SplitDirection;

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

    /// Build the main AppleScript that sets up splits and runs commands.
    /// This runs AFTER the tab has already been created via a separate call.
    fn build_layout_script(path: &Path, panes: &[PaneSpec]) -> Result<String> {
        let path_str = path.to_str().context("invalid worktree path")?;
        let mut lines = Vec::new();

        lines.push(r#"tell application "Ghostty""#.to_string());

        if panes.is_empty() {
            // No panes to configure — just cd to the directory
            lines.push("    set t to focused terminal of selected tab of front window".to_string());
            let escaped_path = escape_applescript(path_str);
            lines.push(format!("    input text \"cd {escaped_path}\" to t"));
            lines.push("    send key \"enter\" to t".to_string());
            lines.push("end tell".to_string());
            return Ok(lines.join("\n"));
        }

        // Create the base configuration with working directory (used for splits)
        lines.push("    set cfg to new surface configuration".to_string());
        lines.push(format!(
            "    set initial working directory of cfg to \"{}\"",
            escape_applescript(path_str)
        ));

        // Get the first pane — it's the terminal in the tab we just created
        let first = &panes[0];
        let first_var = pane_var(&first.name);
        lines.push(format!(
            "    set {first_var} to focused terminal of selected tab of front window"
        ));

        // cd the first pane to the worktree (since new tab didn't get a configuration)
        let escaped_path = escape_applescript(path_str);
        lines.push(format!(
            "    input text \"cd {escaped_path}\" to {first_var}"
        ));
        lines.push(format!("    send key \"enter\" to {first_var}"));

        // Process remaining panes — create splits
        for pane in &panes[1..] {
            let cur_var = pane_var(&pane.name);
            let split_from = pane
                .split_from
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("pane '{}' has no split_from", pane.name))?;
            let parent_var = pane_var(split_from);

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
                let pane_cfg_var = format!("cfg_{cur_var}");
                lines.push(format!(
                    "    set {pane_cfg_var} to new surface configuration"
                ));
                lines.push(format!(
                    "    set initial working directory of {pane_cfg_var} to \"{}\"",
                    escape_applescript(path_str)
                ));
                let env_list = Self::build_ghostty_env_list(&pane.env);
                lines.push(format!(
                    "    set environment variables of {pane_cfg_var} to {env_list}"
                ));
                lines.push(format!(
                    "    set {cur_var} to split {parent_var} direction {dir_str} with configuration {pane_cfg_var}"
                ));
            } else {
                lines.push(format!(
                    "    set {cur_var} to split {parent_var} direction {dir_str} with configuration cfg"
                ));
            }
        }

        // Send commands to each pane
        for pane in panes {
            if let Some(ref cmd) = pane.command
                && !cmd.is_empty()
            {
                let cur_var = pane_var(&pane.name);
                // If pane has env vars and it's the first pane (which didn't get
                // a surface configuration), export them manually
                if pane.split_from.is_none() && !pane.env.is_empty() {
                    for (k, v) in &pane.env {
                        let escaped_k = escape_applescript(k);
                        let escaped_v = escape_applescript(v);
                        lines.push(format!(
                            "    input text \"export {escaped_k}='{escaped_v}'\" to {cur_var}"
                        ));
                        lines.push(format!("    send key \"enter\" to {cur_var}"));
                    }
                }
                let escaped_cmd = escape_applescript(cmd);
                lines.push(format!("    input text \"{escaped_cmd}\" to {cur_var}"));
                lines.push(format!("    send key \"enter\" to {cur_var}"));
            }
        }

        // Focus the first pane
        lines.push(format!("    focus {first_var}"));

        lines.push("end tell".to_string());

        Ok(lines.join("\n"))
    }

    /// Build an AppleScript list literal for environment variables.
    fn build_ghostty_env_list(env: &HashMap<String, String>) -> String {
        let items: Vec<String> = env
            .iter()
            .map(|(k, v)| format!("\"{}={}\"", escape_applescript(k), escape_applescript(v)))
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
        run_applescript_ignoring_errors(r#"tell application "Ghostty" to new tab"#)?;

        // Brief pause to let the tab finish creating
        std::thread::sleep(std::time::Duration::from_millis(500));

        // Step 2: Build and run the layout script (splits, commands)
        let script = Self::build_layout_script(path, panes)?;

        if verbose {
            eprintln!("Setting up workspace layout...");
        }

        run_applescript(&script)?;

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
        let escaped_path = escape_applescript(tab_id);
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

        let _ = run_applescript(&script);
        Ok(())
    }

    fn run_in_pane(&self, tab_id: &str, pane_index: usize, command: &str) -> Result<()> {
        if tab_id.is_empty() {
            return Ok(());
        }

        let escaped_path = escape_applescript(tab_id);
        let escaped_cmd = escape_applescript(command);
        // AppleScript uses 1-based indexing
        let as_index = pane_index + 1;

        let script = format!(
            r#"tell application "Ghostty"
    repeat with w in windows
        repeat with t in tabs of w
            set terms to terminals of t
            repeat with term in terms
                if working directory of term contains "{escaped_path}" then
                    set targetTerm to item {as_index} of terms
                    input text "{escaped_cmd}" to targetTerm
                    send key "enter" to targetTerm
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

        let escaped_path = escape_applescript(tab_id);
        let script = format!(
            r#"tell application "Ghostty"
    activate
    repeat with w in windows
        repeat with t in tabs of w
            set terms to terminals of t
            repeat with term in terms
                if working directory of term contains "{escaped_path}" then
                    select tab t
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
