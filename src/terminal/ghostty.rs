use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use crate::config::types::SplitDirection;
use super::TerminalBackend;

pub struct GhosttyBackend;

impl GhosttyBackend {
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
}

impl TerminalBackend for GhosttyBackend {
    fn open_tab(&self, path: &Path) -> Result<String> {
        let path_str = path.to_str().context("invalid path")?;

        // TODO: Research Ghostty .sdef for exact tab creation API
        let script = r#"tell application "Ghostty"
    tell front window
        make new tab
    end tell
end tell"#;
        let result = Self::run_applescript(script)?;

        // Send cd command to the new tab
        self.run_command(&result, &format!("cd {path_str}"), &HashMap::new())?;

        Ok(result)
    }

    fn split_pane(&self, _target: &str, direction: &SplitDirection) -> Result<String> {
        let dir_str = match direction {
            SplitDirection::Right => "right",
            SplitDirection::Down => "down",
        };

        // TODO: Research Ghostty .sdef for split API
        let script = format!(
            r#"tell application "Ghostty"
    tell front window
        tell current tab
            make new split at end of splits with properties {{direction:"{dir_str}"}}
        end tell
    end tell
end tell"#
        );
        let result = Self::run_applescript(&script)?;
        Ok(result)
    }

    fn run_command(
        &self,
        _target: &str,
        command: &str,
        env: &HashMap<String, String>,
    ) -> Result<()> {
        let full_command = if env.is_empty() {
            command.to_string()
        } else {
            let env_prefix: Vec<String> = env
                .iter()
                .map(|(k, v)| {
                    let escaped_v = v.replace('\'', "'\\''");
                    format!("export {k}='{escaped_v}'")
                })
                .collect();
            format!("{} && {command}", env_prefix.join(" && "))
        };

        // TODO: Research Ghostty .sdef for sending text to panes
        let escaped = full_command.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!(
            r#"tell application "Ghostty"
    tell front window
        tell current tab
            tell current split
                write "{escaped}"
            end tell
        end tell
    end tell
end tell"#
        );
        Self::run_applescript(&script)?;
        Ok(())
    }

    fn close_tab(&self, tab_id: &str) -> Result<()> {
        if tab_id.is_empty() {
            return Ok(());
        }

        // TODO: Research Ghostty .sdef for closing specific tabs
        let script = r#"tell application "Ghostty"
    tell front window
        tell current tab
            close
        end tell
    end tell
end tell"#;
        let _ = Self::run_applescript(script);
        Ok(())
    }
}
