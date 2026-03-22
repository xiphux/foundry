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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_applescript_backslash() {
        assert_eq!(escape_applescript(r"a\b"), r"a\\b");
    }

    #[test]
    fn escape_applescript_double_quote() {
        assert_eq!(escape_applescript(r#"say "hi""#), r#"say \"hi\""#);
    }

    #[test]
    fn escape_applescript_mixed() {
        assert_eq!(escape_applescript(r#"a\"b"#), r#"a\\\"b"#);
    }

    #[test]
    fn escape_applescript_no_escape_needed() {
        assert_eq!(escape_applescript("hello world"), "hello world");
    }

    #[test]
    fn pane_var_alphanumeric() {
        assert_eq!(pane_var("editor"), "pane_editor");
    }

    #[test]
    fn pane_var_special_chars_become_underscores() {
        assert_eq!(pane_var("my-pane.1"), "pane_my_pane_1");
    }

    #[test]
    fn pane_var_spaces() {
        assert_eq!(pane_var("my pane"), "pane_my_pane");
    }
}
