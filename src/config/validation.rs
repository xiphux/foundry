use anyhow::{Result, bail};

/// Reject an environment variable name that is not a plain shell identifier.
///
/// Env names are not just data: every terminal backend builds `export NAME=...`
/// (or `$env:NAME = ...`) by hand and interpolates the name into a shell line
/// unescaped, because a name is expected to be an identifier. A name carrying
/// `;` or a newline would therefore end the export statement and start a
/// command. Values go through `terminal::shell_export`, which quotes and
/// escapes them, so the name is the side that needs constraining here.
///
/// Both sources of env names are repo-settable — `panes.<name>.env` keys and
/// `ports` entries, the latter becoming a name in every pane — so this runs at
/// config load, which is the one place both funnel through, rather than at each
/// of the six places a backend emits an export.
pub fn validate_env_name(name: &str, context: &str) -> Result<()> {
    if name.is_empty() {
        bail!("empty environment variable name in {context}");
    }
    let valid_start = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    let valid_rest = name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !valid_start || !valid_rest {
        bail!(
            "invalid environment variable name {name:?} in {context}. \
             Names must match [A-Za-z_][A-Za-z0-9_]*."
        );
    }
    Ok(())
}

/// Reject a repo-supplied path that carries shell metacharacters.
///
/// `worktree_dir` is settable from `.foundry.toml`, which the repository
/// controls, and the resulting worktree path is typed into a live shell by the
/// AppleScript backends as `cd <path>`. That `cd` is quoted now, so this is the
/// second line of defence rather than the only one — but a repo has no reason
/// to ask for a directory named with a `;` or a newline in it, and refusing
/// outright beats relying on every future emission site to quote correctly.
///
/// Deliberately not a trust prompt: a path is data, not a command, and
/// prompting for every project that merely relocates its worktrees would train
/// people to click through the prompt that guards the things that *are*
/// commands.
///
/// The set is only what could end a command or start a substitution if a
/// future emission site forgot to quote. Everything else stays legal because
/// real directories contain it: spaces and `(`/`)` (Dropbox creates
/// `~/Dropbox (Personal)`; Windows has `C:\Program Files (x86)`), backslashes
/// for Windows paths, and `*`/`?`/`<`/`>` which are inert inside quotes.
/// Rejecting those would lock a user out of every command — including the
/// `discard` they would need to recover — for a path that was never dangerous.
pub fn validate_project_path(value: &str, context: &str) -> Result<()> {
    const FORBIDDEN: &[char] = &[';', '&', '|', '$', '`', '"', '\''];

    if let Some(bad) = value
        .chars()
        .find(|c| c.is_control() || FORBIDDEN.contains(c))
    {
        bail!(
            "{context} contains an unsupported character {bad:?}: {value:?}. \
             A path set by a project config cannot contain shell metacharacters."
        );
    }
    Ok(())
}

/// Known top-level keys for the global config file.
const GLOBAL_CONFIG_KEYS: &[&str] = &[
    "branch_prefix",
    "agent",
    "agent_command",
    "archive_prefix",
    "merge_strategy",
    "worktree_dir",
    "port_range_start",
    "auto_fetch",
    "fetch_remote",
    "pr_remote",
    "issue_prompt",
    "unrestricted_permissions",
    "editor",
    "shell",
    "panes",
];

/// Known top-level keys for the project config file.
const PROJECT_CONFIG_KEYS: &[&str] = &[
    "branch_prefix",
    "agent",
    "agent_command",
    "archive_prefix",
    "merge_strategy",
    "worktree_dir",
    "auto_fetch",
    "fetch_remote",
    "pr_remote",
    "unrestricted_permissions",
    "shell",
    "scripts",
    "panes",
    "ports",
    "context",
];

/// Known keys for `[[panes]]` entries in the global config.
const PANE_CONFIG_KEYS: &[&str] = &[
    "name",
    "agent",
    "command",
    "split_from",
    "direction",
    "optional",
    "env",
    "deferred",
];

/// Known keys for `[panes.<name>]` overrides in the project config.
const PANE_OVERRIDE_KEYS: &[&str] = &["agent", "command", "env", "deferred"];

/// Known keys for `[[scripts.setup]]` and `[[scripts.teardown]]` entries.
const SCRIPT_CONFIG_KEYS: &[&str] = &["name", "command", "working_dir", "deferred"];

/// Known keys for the `[scripts]` table.
const SCRIPTS_CONFIG_KEYS: &[&str] = &["setup", "teardown"];

/// Warn about unrecognized keys in a TOML table.
///
/// The key name comes from the file being validated, which for `.foundry.toml`
/// means the repository chooses it — and TOML allows a quoted key to carry a
/// `` escape. This runs immediately before the trust prompt is drawn, so an
/// unsanitized key would hand a repo the very cursor-control primitive that
/// prompt sanitizes its own text against.
fn warn_unknown_keys(table: &toml::value::Table, known: &[&str], context: &str) {
    for key in table.keys() {
        if !known.contains(&key.as_str()) {
            let key = crate::str_util::sanitize_for_display(key);
            eprintln!("Warning: unknown config key '{key}' in {context} (ignored)");
        }
    }
}

/// Check a parsed TOML value for unknown keys in the global config schema.
pub fn check_global_config_keys(value: &toml::Value, file_path: &str) {
    let Some(table) = value.as_table() else {
        return;
    };
    warn_unknown_keys(table, GLOBAL_CONFIG_KEYS, file_path);

    // Check [[panes]] entries
    if let Some(toml::Value::Array(panes)) = table.get("panes") {
        for (i, pane) in panes.iter().enumerate() {
            if let Some(pane_table) = pane.as_table() {
                let fallback = format!("index {i}");
                let name = pane_table
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&fallback);
                warn_unknown_keys(
                    pane_table,
                    PANE_CONFIG_KEYS,
                    &format!("{file_path} pane '{name}'"),
                );
            }
        }
    }
}

/// Check a parsed TOML value for unknown keys in the project config schema.
pub fn check_project_config_keys(value: &toml::Value, file_path: &str) {
    let Some(table) = value.as_table() else {
        return;
    };
    warn_unknown_keys(table, PROJECT_CONFIG_KEYS, file_path);

    // Check [scripts] table
    if let Some(toml::Value::Table(scripts)) = table.get("scripts") {
        warn_unknown_keys(
            scripts,
            SCRIPTS_CONFIG_KEYS,
            &format!("{file_path} [scripts]"),
        );

        // Check [[scripts.setup]] and [[scripts.teardown]] entries
        for section in ["setup", "teardown"] {
            if let Some(toml::Value::Array(entries)) = scripts.get(section) {
                for (i, entry) in entries.iter().enumerate() {
                    if let Some(entry_table) = entry.as_table() {
                        let fallback = format!("index {i}");
                        let name = entry_table
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or(&fallback);
                        warn_unknown_keys(
                            entry_table,
                            SCRIPT_CONFIG_KEYS,
                            &format!("{file_path} {section} script '{name}'"),
                        );
                    }
                }
            }
        }
    }

    // Check [panes.<name>] overrides
    if let Some(toml::Value::Table(panes)) = table.get("panes") {
        for (pane_name, pane_value) in panes {
            if let Some(pane_table) = pane_value.as_table() {
                warn_unknown_keys(
                    pane_table,
                    PANE_OVERRIDE_KEYS,
                    &format!("{file_path} pane override '{pane_name}'"),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_global_config_keys_no_warnings_for_valid() {
        let toml_str = r#"
            branch_prefix = "user"
            agent = "claude"
            archive_prefix = "archive"
            merge_strategy = "ff-only"
            worktree_dir = "~/.foundry/worktrees"
            auto_fetch = false
        "#;
        let value: toml::Value = toml_str.parse().unwrap();
        check_global_config_keys(&value, "test");
    }

    #[test]
    fn check_global_config_keys_detects_unknown() {
        let toml_str = r#"
            agent = "claude"
            branchprefix = "typo"
        "#;
        let value: toml::Value = toml_str.parse().unwrap();
        check_global_config_keys(&value, "test");
    }

    #[test]
    fn check_project_config_keys_no_warnings_for_valid() {
        let toml_str = r#"
            agent = "codex"
            ports = ["VITE_PORT"]

            [scripts]
            [[scripts.setup]]
            name = "install"
            command = "npm install"
            deferred = true

            [panes.server]
            command = "npm run dev"
        "#;
        let value: toml::Value = toml_str.parse().unwrap();
        check_project_config_keys(&value, "test");
    }

    #[test]
    fn check_project_config_keys_detects_unknown_in_script() {
        let toml_str = r#"
            [[scripts.setup]]
            name = "install"
            command = "npm install"
            timeout = 30
        "#;
        let value: toml::Value = toml_str.parse().unwrap();
        check_project_config_keys(&value, "test");
    }

    #[test]
    fn check_project_config_keys_detects_unknown_in_pane_override() {
        let toml_str = r#"
            [panes.shell]
            command = "bash"
            split_from = "agent"
        "#;
        let value: toml::Value = toml_str.parse().unwrap();
        check_project_config_keys(&value, "test");
    }

    #[test]
    fn validate_env_name_accepts_identifiers() {
        for name in ["PORT", "VITE_PORT", "_x", "A1", "a_b_c9"] {
            assert!(
                validate_env_name(name, "test").is_ok(),
                "should accept {name:?}"
            );
        }
    }

    /// Names are interpolated into `export NAME=...` unescaped, so anything
    /// that could end the statement has to be refused here.
    #[test]
    fn validate_env_name_rejects_shell_metacharacters() {
        for name in [
            "A; touch /tmp/pwned; B",
            "A B",
            "A\nB",
            "A$(id)",
            "A`id`",
            "A|B",
            "A&B",
            "A'B",
            "A\"B",
            "A=B",
            "A-B",
        ] {
            assert!(
                validate_env_name(name, "test").is_err(),
                "should reject {name:?}"
            );
        }
    }

    #[test]
    fn validate_env_name_rejects_empty_leading_digit_and_non_ascii() {
        assert!(validate_env_name("", "test").is_err());
        assert!(validate_env_name("1PORT", "test").is_err());
        // Unicode letters are alphanumeric but are not shell identifiers.
        assert!(validate_env_name("PÖRT", "test").is_err());
        assert!(validate_env_name("日本", "test").is_err());
    }

    #[test]
    fn validate_project_path_accepts_real_paths() {
        for p in [
            "/home/u/worktrees",
            "~/.foundry/worktrees",
            "/tmp/My Projects/wt",
            "/Users/me/Dropbox (Personal)/wt",
            "C:/Program Files (x86)/wt",
            "/tmp/glob[1]/wt",
            "/tmp/a?b/wt",
            "C:\\\\Users\\\\me\\\\wt",
            "relative/path",
        ] {
            assert!(
                validate_project_path(p, "test").is_ok(),
                "should accept {p:?}"
            );
        }
    }

    /// The worktree path is typed into a shell as `cd <path>` by the AppleScript
    /// backends, so a repo must not be able to smuggle a command into it.
    #[test]
    fn validate_project_path_rejects_shell_metacharacters() {
        for p in [
            "/tmp/a; touch /tmp/pwned; b",
            "/tmp/$(id)",
            "/tmp/`id`",
            "/tmp/a|b",
            "/tmp/a&b",
            "/tmp/a\nb",
            "/tmp/a'b",
            "/tmp/a\"b",
            "/tmp/a\u{7f}b",
        ] {
            assert!(
                validate_project_path(p, "test").is_err(),
                "should reject {p:?}"
            );
        }
    }

    #[test]
    fn warn_unknown_keys_finds_extras() {
        let mut table = toml::value::Table::new();
        table.insert("known".into(), toml::Value::String("ok".into()));
        table.insert("typo".into(), toml::Value::String("bad".into()));
        warn_unknown_keys(&table, &["known"], "test");
    }
}
