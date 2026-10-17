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

/// Escape a line that will be *typed* into a shell (`input text`, `write text`).
///
/// Typed text goes through the line editor before the shell parses anything,
/// so a control character is a keystroke, not data — and pane commands carry
/// the agent prompt, which under `--issue` is GitHub issue text. Ctrl-A, for
/// one, jumps to the start of the line, so a body beginning `\x01rm -rf ~; `
/// put that ahead of `claude '...'` and ran it on Enter, quoting or no
/// quoting. Controls are dropped; tab (which would trigger completion) becomes
/// a space and CR a newline. Newline itself stays: inside the single-quoted
/// prompt a typed newline only continues the quoted string.
pub fn escape_typed_input(s: &str) -> String {
    let typed: String = s
        .chars()
        .filter_map(|c| match c {
            '\n' => Some('\n'),
            '\r' => Some('\n'),
            '\t' => Some(' '),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect();
    escape_applescript(&typed)
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
    fn escape_typed_input_drops_line_editor_keystrokes() {
        assert_eq!(
            escape_typed_input("claude '\x01rm -rf ~; \x1b[A\u{7f}\u{9b}x'"),
            "claude 'rm -rf ~; [Ax'"
        );
        assert_eq!(
            escape_typed_input("a\tb\r\nc\n\"d\""),
            "a b\n\nc\n\\\"d\\\""
        );
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
