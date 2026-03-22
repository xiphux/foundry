use anyhow::{Context, Result};
use std::process::Command;

/// Run an AppleScript and return its stdout.
pub fn run_applescript(script: &str) -> Result<String> {
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

/// Run an AppleScript, ignoring errors. Used for commands that succeed
/// but throw spurious errors (e.g., Ghostty 1.x `new tab`).
pub fn run_applescript_ignoring_errors(script: &str) -> Result<()> {
    let _ = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .context("failed to run osascript")?;
    Ok(())
}

/// Escape a string for use inside AppleScript double-quoted strings.
pub fn escape_applescript(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Build a sanitized AppleScript variable name for a pane.
pub fn pane_var(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    format!("pane_{sanitized}")
}
