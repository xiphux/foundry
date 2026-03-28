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
fn warn_unknown_keys(table: &toml::value::Table, known: &[&str], context: &str) {
    for key in table.keys() {
        if !known.contains(&key.as_str()) {
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
    fn warn_unknown_keys_finds_extras() {
        let mut table = toml::value::Table::new();
        table.insert("known".into(), toml::Value::String("ok".into()));
        table.insert("typo".into(), toml::Value::String("bad".into()));
        warn_unknown_keys(&table, &["known"], "test");
    }
}
