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
            lines.push("    return id of selected tab of front window".to_string());
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

        // Hand back the tab's id so later operations can find this tab again.
        lines.push("    return id of selected tab of front window".to_string());

        lines.push("end tell".to_string());

        Ok(lines.join("\n"))
    }

    /// Wrap `body` in a loop that binds `t` to the tab whose id is `tab_id`.
    ///
    /// Tabs are located by the id captured when the tab was created. Ghostty's
    /// `working directory` property on a terminal is declared in its scripting
    /// dictionary but never populated (empty on 1.3.1 even with shell
    /// integration active), so matching on the worktree path never fires.
    fn with_tab(tab_id: &str, body: &str) -> String {
        let escaped_id = escape_applescript(tab_id);
        format!(
            r#"tell application "Ghostty"
    repeat with w in windows
        repeat with t in tabs of w
            if id of t is "{escaped_id}" then
{body}
            end if
        end repeat
    end repeat
    return "not_found"
end tell"#
        )
    }

    fn build_close_script(tab_id: &str) -> String {
        Self::with_tab(
            tab_id,
            "                close tab t\n                return \"found\"",
        )
    }

    fn build_run_in_pane_script(tab_id: &str, pane_index: usize, command: &str) -> String {
        let escaped_cmd = escape_applescript(command);
        // AppleScript uses 1-based indexing
        let as_index = pane_index + 1;
        Self::with_tab(
            tab_id,
            &format!(
                r#"                set targetTerm to item {as_index} of (terminals of t)
                input text "{escaped_cmd}" to targetTerm
                send key "enter" to targetTerm
                return "found""#
            ),
        )
    }

    fn build_focus_script(tab_id: &str) -> String {
        let escaped_id = escape_applescript(tab_id);
        format!(
            r#"tell application "Ghostty"
    activate
    repeat with w in windows
        repeat with t in tabs of w
            if id of t is "{escaped_id}" then
                select tab t
                return "found"
            end if
        end repeat
    end repeat
    return "not_found"
end tell"#
        )
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
    fn settle_delay(&self) -> std::time::Duration {
        std::time::Duration::from_millis(500)
    }

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

        // The layout script hands back the new tab's id, which is the handle
        // every later operation (close, focus, run_in_pane) uses.
        let tab_id = run_applescript(&script)?;

        if tab_id.is_empty() {
            eprintln!(
                "warning: Ghostty did not report a tab id; \
                 this workspace's tab cannot be closed or focused automatically"
            );
        }

        Ok(tab_id)
    }

    fn close_tab(&self, tab_id: &str) -> Result<()> {
        if tab_id.is_empty() {
            return Ok(());
        }

        // Ghostty prompts before closing a tab whose panes still have running
        // processes (`confirm-close-surface`, on by default), so the tab may
        // linger until the user confirms.
        let _ = run_applescript(&Self::build_close_script(tab_id));
        Ok(())
    }

    fn run_in_pane(&self, tab_id: &str, pane_index: usize, command: &str) -> Result<()> {
        if tab_id.is_empty() {
            return Ok(());
        }

        let result = run_applescript(&Self::build_run_in_pane_script(tab_id, pane_index, command))?;
        if result == "not_found" {
            anyhow::bail!("Ghostty tab '{tab_id}' not found; cannot run command in pane");
        }
        Ok(())
    }

    fn focus_tab(&self, tab_id: &str) -> Result<bool> {
        if tab_id.is_empty() {
            return Ok(false);
        }

        let result = run_applescript(&Self::build_focus_script(tab_id))?;
        Ok(result == "found")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panes() -> Vec<PaneSpec> {
        vec![PaneSpec {
            name: "agent".into(),
            split_from: None,
            direction: None,
            command: Some("claude".into()),
            env: HashMap::new(),
            shell: None,
        }]
    }

    #[test]
    fn layout_script_returns_the_tab_id() {
        let script = GhosttyBackend::build_layout_script(Path::new("/tmp/wt"), &panes()).unwrap();
        assert!(script.contains("return id of selected tab of front window"));
    }

    #[test]
    fn layout_script_without_panes_returns_the_tab_id() {
        let script = GhosttyBackend::build_layout_script(Path::new("/tmp/wt"), &[]).unwrap();
        assert!(script.contains("return id of selected tab of front window"));
    }

    /// Ghostty never populates `working directory`, so no script may depend on it.
    #[test]
    fn scripts_never_match_on_working_directory() {
        let scripts = [
            GhosttyBackend::build_close_script("tab-abc"),
            GhosttyBackend::build_run_in_pane_script("tab-abc", 1, "ls"),
            GhosttyBackend::build_focus_script("tab-abc"),
        ];
        for script in &scripts {
            assert!(
                !script.contains("working directory"),
                "script must not match on working directory: {script}"
            );
            assert!(script.contains(r#"id of t is "tab-abc""#), "{script}");
        }
    }

    #[test]
    fn close_script_closes_the_tab_not_a_pane() {
        let script = GhosttyBackend::build_close_script("tab-abc");
        assert!(script.contains("close tab t"));
    }

    #[test]
    fn run_in_pane_script_uses_one_based_index() {
        let script = GhosttyBackend::build_run_in_pane_script("tab-abc", 2, "pnpm install");
        assert!(script.contains("item 3 of (terminals of t)"));
        assert!(script.contains(r#"input text "pnpm install" to targetTerm"#));
    }

    #[test]
    fn scripts_escape_quotes_in_commands() {
        let script = GhosttyBackend::build_run_in_pane_script("tab-abc", 0, r#"echo "hi""#);
        assert!(script.contains(r#"echo \"hi\""#));
    }

    #[test]
    fn lookup_scripts_report_not_found() {
        assert!(GhosttyBackend::build_focus_script("tab-abc").contains(r#"return "not_found""#));
        assert!(
            GhosttyBackend::build_run_in_pane_script("tab-abc", 0, "ls")
                .contains(r#"return "not_found""#)
        );
    }
}
