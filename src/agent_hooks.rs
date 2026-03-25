use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::config;

/// The possible agent statuses we track.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AgentStatus {
    Working,
    Idle,
    WaitingPermission,
    Unknown,
}

impl AgentStatus {
    pub fn label(self) -> &'static str {
        match self {
            AgentStatus::Working => "working",
            AgentStatus::Idle => "idle",
            AgentStatus::WaitingPermission => "waiting for permission",
            AgentStatus::Unknown => "unknown",
        }
    }
}

/// Get the path to the status file for a specific agent in a workspace.
pub fn status_file_path(project: &str, name: &str, agent: &str) -> Result<PathBuf> {
    let foundry_dir = config::foundry_dir()?;
    Ok(foundry_dir
        .join("status")
        .join(project)
        .join(format!("{name}-{agent}.status")))
}

/// Read the current agent status from the status file.
pub fn read_status(project: &str, name: &str, agent: &str) -> AgentStatus {
    let path = match status_file_path(project, name, agent) {
        Ok(p) => p,
        Err(_) => return AgentStatus::Unknown,
    };

    match std::fs::read_to_string(&path) {
        Ok(content) => match content.trim() {
            "working" => AgentStatus::Working,
            "idle" => AgentStatus::Idle,
            "waiting_permission" => AgentStatus::WaitingPermission,
            _ => AgentStatus::Unknown,
        },
        Err(_) => AgentStatus::Unknown,
    }
}

/// Read statuses for all agents in a workspace. Returns a list of (agent_name, status) pairs.
pub fn read_all_statuses(project: &str, name: &str) -> Vec<(String, AgentStatus)> {
    let foundry_dir = match config::foundry_dir() {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };

    let status_dir = foundry_dir.join("status").join(project);
    let prefix = format!("{name}-");
    let suffix = ".status";

    let entries = match std::fs::read_dir(&status_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut statuses = Vec::new();
    for entry in entries.flatten() {
        let filename = entry.file_name().to_string_lossy().to_string();
        if filename.starts_with(&prefix) && filename.ends_with(suffix) {
            let agent = &filename[prefix.len()..filename.len() - suffix.len()];
            if !agent.is_empty() {
                let status = read_status(project, name, agent);
                statuses.push((agent.to_string(), status));
            }
        }
    }
    statuses.sort_by(|a, b| a.0.cmp(&b.0));
    statuses
}

/// Remove all status files for a workspace (cleanup on finish/discard).
pub fn remove_status(project: &str, name: &str) {
    let foundry_dir = match config::foundry_dir() {
        Ok(d) => d,
        Err(_) => return,
    };

    let status_dir = foundry_dir.join("status").join(project);
    let prefix = format!("{name}-");

    if let Ok(entries) = std::fs::read_dir(&status_dir) {
        for entry in entries.flatten() {
            let filename = entry.file_name().to_string_lossy().to_string();
            if filename.starts_with(&prefix) && filename.ends_with(".status") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

/// Compute the Claude project directory path for a worktree.
/// Claude stores conversations at ~/.claude/projects/<mangled-path>/
/// where the path has slashes replaced with dashes.
pub fn claude_project_dir(worktree_path: &Path) -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let abs_path = if worktree_path.is_absolute() {
        worktree_path.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(worktree_path)
    };
    let mangled = abs_path.to_string_lossy().replace(['/', '.'], "-");
    Some(home.join(".claude").join("projects").join(mangled))
}

/// Check if an agent has an existing conversation for a worktree.
/// Currently only Claude stores file-based conversations; other agents
/// always return false.
pub fn has_agent_conversation(agent: &str, worktree_path: &Path) -> bool {
    match agent {
        "claude" => has_claude_conversation(worktree_path),
        _ => false,
    }
}

/// Clear agent conversation state for a worktree.
/// Used when starting a new workspace to prevent resuming stale conversations
/// from a previous workspace with the same name.
pub fn clear_agent_conversations(worktree_path: &Path) {
    // Currently only Claude stores file-based conversation state
    clear_claude_conversations(worktree_path);
}

/// Check if a Claude conversation exists for a worktree (has .jsonl files).
fn has_claude_conversation(worktree_path: &Path) -> bool {
    let dir = match claude_project_dir(worktree_path) {
        Some(d) => d,
        None => return false,
    };

    if !dir.exists() {
        return false;
    }

    std::fs::read_dir(&dir)
        .map(|entries| {
            entries
                .flatten()
                .any(|e| e.path().extension().is_some_and(|ext| ext == "jsonl"))
        })
        .unwrap_or(false)
}

/// Clear the Claude conversation directory for a worktree.
fn clear_claude_conversations(worktree_path: &Path) {
    if let Some(dir) = claude_project_dir(worktree_path) {
        if dir.exists() {
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn agent_status_label_all_variants() {
        assert_eq!(AgentStatus::Working.label(), "working");
        assert_eq!(AgentStatus::Idle.label(), "idle");
        assert_eq!(
            AgentStatus::WaitingPermission.label(),
            "waiting for permission"
        );
        assert_eq!(AgentStatus::Unknown.label(), "unknown");
    }

    #[test]
    fn status_file_path_construction() {
        let path = status_file_path("myproject", "my-workspace", "claude").unwrap();
        assert!(path.ends_with("status/myproject/my-workspace-claude.status"));
        assert!(path.to_string_lossy().contains(".foundry"));
    }

    #[test]
    fn read_status_working() {
        let path = status_file_path("testproj_read2", "testws", "claude").unwrap();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, "working").unwrap();
        assert_eq!(
            read_status("testproj_read2", "testws", "claude"),
            AgentStatus::Working
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn read_status_idle() {
        let path = status_file_path("testproj_idle2", "testws", "claude").unwrap();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, "idle").unwrap();
        assert_eq!(
            read_status("testproj_idle2", "testws", "claude"),
            AgentStatus::Idle
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn read_status_waiting_permission() {
        let path = status_file_path("testproj_wait2", "testws", "claude").unwrap();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, "waiting_permission").unwrap();
        assert_eq!(
            read_status("testproj_wait2", "testws", "claude"),
            AgentStatus::WaitingPermission
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn read_status_missing_file() {
        assert_eq!(
            read_status("nonexistent_proj_xyz2", "nonexistent_ws", "claude"),
            AgentStatus::Unknown
        );
    }

    #[test]
    fn read_status_invalid_content() {
        let path = status_file_path("testproj_invalid2", "testws", "claude").unwrap();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, "bogus_value").unwrap();
        assert_eq!(
            read_status("testproj_invalid2", "testws", "claude"),
            AgentStatus::Unknown
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn remove_status_existing_file() {
        let path = status_file_path("testproj_rm2", "testws", "claude").unwrap();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, "idle").unwrap();
        assert!(path.exists());
        remove_status("testproj_rm2", "testws");
        assert!(!path.exists());
    }

    #[test]
    fn remove_status_nonexistent_file() {
        // Should not panic
        remove_status("nonexistent_proj_rm_xyz2", "nonexistent_ws");
    }

    #[test]
    fn merge_hooks_adds_to_empty() {
        let existing = serde_json::json!({});
        let foundry = serde_json::json!({
            "Stop": [{"matcher": "*", "hooks": [{"type": "command", "command": "echo idle"}]}]
        });
        let merged = merge_hooks(&existing, &foundry);
        assert!(merged.get("Stop").unwrap().as_array().unwrap().len() == 1);
    }

    #[test]
    fn merge_hooks_appends_to_existing() {
        let existing = serde_json::json!({
            "Stop": [{"matcher": "*", "hooks": [{"type": "command", "command": "echo user_hook"}]}]
        });
        let foundry = serde_json::json!({
            "Stop": [{"matcher": "*", "hooks": [{"type": "command", "command": "echo idle"}]}]
        });
        let merged = merge_hooks(&existing, &foundry);
        // Should have both hooks
        assert_eq!(merged.get("Stop").unwrap().as_array().unwrap().len(), 2);
    }

    #[test]
    fn merge_hooks_preserves_unrelated_events() {
        let existing = serde_json::json!({
            "PostToolUse": [{"matcher": "Edit", "hooks": [{"type": "command", "command": "cargo fmt"}]}]
        });
        let foundry = serde_json::json!({
            "Stop": [{"matcher": "*", "hooks": [{"type": "command", "command": "echo idle"}]}]
        });
        let merged = merge_hooks(&existing, &foundry);
        assert!(merged.get("PostToolUse").is_some());
        assert!(merged.get("Stop").is_some());
    }

    #[test]
    fn merge_permissions_adds_new_entries() {
        let existing = vec![serde_json::json!("Bash(pnpm *)")];
        let additions = vec!["Read(/tmp/**)".into(), "Edit(/tmp/**)".into()];
        let merged = merge_permissions(&existing, &additions, &[]);
        assert_eq!(merged.len(), 3);
    }

    #[test]
    fn merge_permissions_deduplicates() {
        let existing = vec![serde_json::json!("Bash(git add:*)")];
        let additions = vec!["Bash(git add:*)".into()];
        let merged = merge_permissions(&existing, &additions, &[]);
        assert_eq!(merged.len(), 1);
    }

    #[test]
    fn merge_permissions_strips_patterns() {
        let existing = vec![
            serde_json::json!("Bash(git push *)"),
            serde_json::json!("Bash(pnpm *)"),
            serde_json::json!("Bash(git push --force)"),
        ];
        let merged = merge_permissions(&existing, &[], &["git push"]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0], serde_json::json!("Bash(pnpm *)"));
    }

    #[test]
    fn build_worktree_permissions_scoped_to_path() {
        let path = std::path::PathBuf::from("/tmp/worktrees/myproject/feature");
        let (allow, deny) = build_worktree_permissions(&path);
        assert!(allow
            .iter()
            .any(|a| a.contains("/tmp/worktrees/myproject/feature/**")));
        assert!(allow.iter().any(|a| a.starts_with("Read(")));
        assert!(allow.iter().any(|a| a.starts_with("Edit(")));
        assert!(allow.iter().any(|a| a.starts_with("Write(")));
        assert!(deny.iter().any(|d| d.contains("git push")));
        assert!(deny.iter().any(|d| d.contains("checkout main")));
    }

    #[test]
    fn setup_agent_hooks_copies_source_settings() {
        let source = TempDir::new().unwrap();
        let worktree = TempDir::new().unwrap();

        // Create source settings.local.json with existing permissions
        let source_claude = source.path().join(".claude");
        std::fs::create_dir_all(&source_claude).unwrap();
        std::fs::write(
            source_claude.join("settings.local.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "permissions": {
                    "allow": ["Bash(pnpm *)"]
                }
            }))
            .unwrap(),
        )
        .unwrap();

        setup_agent_hooks(worktree.path(), source.path(), "test_copy", "ws", "claude").unwrap();

        let settings_path = worktree.path().join(".claude").join("settings.local.json");
        let content = std::fs::read_to_string(&settings_path).unwrap();
        let settings: serde_json::Value = serde_json::from_str(&content).unwrap();

        // Should have the copied pnpm permission
        let allow = settings["permissions"]["allow"].as_array().unwrap();
        assert!(allow.iter().any(|v| v.as_str() == Some("Bash(pnpm *)")));

        // Should also have worktree-scoped permissions
        let wt_str = worktree.path().to_string_lossy();
        assert!(allow.iter().any(|v| {
            v.as_str()
                .map(|s| s.contains(&*wt_str) && s.starts_with("Read("))
                .unwrap_or(false)
        }));

        // Should have hooks
        assert!(settings.get("hooks").is_some());
        assert!(settings["hooks"].get("Stop").is_some());

        // Cleanup
        let _ = remove_status("test_copy", "ws");
    }

    #[test]
    fn setup_agent_hooks_strips_git_push_from_source() {
        let source = TempDir::new().unwrap();
        let worktree = TempDir::new().unwrap();

        let source_claude = source.path().join(".claude");
        std::fs::create_dir_all(&source_claude).unwrap();
        std::fs::write(
            source_claude.join("settings.local.json"),
            serde_json::to_string_pretty(&serde_json::json!({
                "permissions": {
                    "allow": ["Bash(git push *)", "Bash(pnpm *)"]
                }
            }))
            .unwrap(),
        )
        .unwrap();

        setup_agent_hooks(worktree.path(), source.path(), "test_strip", "ws", "claude").unwrap();

        let settings_path = worktree.path().join(".claude").join("settings.local.json");
        let content = std::fs::read_to_string(&settings_path).unwrap();
        let settings: serde_json::Value = serde_json::from_str(&content).unwrap();

        let allow = settings["permissions"]["allow"].as_array().unwrap();
        // git push should be stripped
        assert!(!allow
            .iter()
            .any(|v| v.as_str().map(|s| s.contains("git push")).unwrap_or(false)));
        // pnpm should remain
        assert!(allow.iter().any(|v| v.as_str() == Some("Bash(pnpm *)")));

        let _ = remove_status("test_strip", "ws");
    }

    #[test]
    fn setup_agent_hooks_skips_for_codex() {
        let source = TempDir::new().unwrap();
        let worktree = TempDir::new().unwrap();

        setup_agent_hooks(worktree.path(), source.path(), "test_codex", "ws", "codex").unwrap();

        // Should NOT create .claude/settings.local.json for codex
        let settings_path = worktree.path().join(".claude").join("settings.local.json");
        assert!(!settings_path.exists());
    }

    #[test]
    fn setup_agent_hooks_skips_for_unknown_agent() {
        let source = TempDir::new().unwrap();
        let worktree = TempDir::new().unwrap();

        setup_agent_hooks(
            worktree.path(),
            source.path(),
            "test_other",
            "ws",
            "some-agent",
        )
        .unwrap();

        let settings_path = worktree.path().join(".claude").join("settings.local.json");
        assert!(!settings_path.exists());
    }

    #[test]
    fn claude_project_dir_mangles_path() {
        let path = std::path::PathBuf::from("/Users/test/.foundry/worktrees/myapp/feature");
        let dir = claude_project_dir(&path).unwrap();
        let dir_name = dir.file_name().unwrap().to_string_lossy();
        assert_eq!(dir_name, "-Users-test--foundry-worktrees-myapp-feature");
    }

    #[test]
    fn has_agent_conversation_false_when_no_dir() {
        let dir = TempDir::new().unwrap();
        assert!(!has_agent_conversation("claude", dir.path()));
    }

    #[test]
    fn has_agent_conversation_true_when_jsonl_exists() {
        let dir = TempDir::new().unwrap();
        if let Some(project_dir) = claude_project_dir(dir.path()) {
            std::fs::create_dir_all(&project_dir).unwrap();
            std::fs::write(project_dir.join("abc123.jsonl"), "test conversation").unwrap();
            assert!(has_agent_conversation("claude", dir.path()));
            let _ = std::fs::remove_dir_all(&project_dir);
        }
    }

    #[test]
    fn has_agent_conversation_false_for_unknown_agent() {
        let dir = TempDir::new().unwrap();
        assert!(!has_agent_conversation("codex", dir.path()));
        assert!(!has_agent_conversation("unknown-agent", dir.path()));
    }

    #[test]
    fn clear_agent_conversations_removes_dir() {
        let dir = TempDir::new().unwrap();
        if let Some(project_dir) = claude_project_dir(dir.path()) {
            std::fs::create_dir_all(&project_dir).unwrap();
            std::fs::write(project_dir.join("abc123.jsonl"), "test conversation").unwrap();
            assert!(project_dir.exists());
            clear_agent_conversations(dir.path());
            assert!(!project_dir.exists());
        }
    }

    #[test]
    fn clear_agent_conversations_noop_when_no_dir() {
        let dir = TempDir::new().unwrap();
        // Should not panic
        clear_agent_conversations(dir.path());
    }
}

/// Build the foundry-specific hooks for agent status tracking.
fn build_status_hooks(status_path_str: &str) -> serde_json::Value {
    serde_json::json!({
        "UserPromptSubmit": [
            {
                "matcher": "*",
                "hooks": [
                    {
                        "type": "command",
                        "command": format!("echo working > '{status_path_str}'"),
                        "timeout": 5
                    }
                ]
            }
        ],
        "Stop": [
            {
                "matcher": "*",
                "hooks": [
                    {
                        "type": "command",
                        "command": format!("echo idle > '{status_path_str}'"),
                        "timeout": 5
                    }
                ]
            }
        ],
        "Notification": [
            {
                "matcher": "permission_prompt",
                "hooks": [
                    {
                        "type": "command",
                        "command": format!("echo waiting_permission > '{status_path_str}'"),
                        "timeout": 5
                    }
                ]
            }
        ]
    })
}

/// Build worktree-scoped permissions. Allows unrestricted file operations
/// within the worktree and safe git operations on the current branch.
/// Denies push and checkout of main/master.
fn build_worktree_permissions(worktree_path: &Path) -> (Vec<String>, Vec<String>) {
    let wt = worktree_path.to_string_lossy();
    let allow = vec![
        format!("Read({wt}/**)"),
        format!("Edit({wt}/**)"),
        format!("Write({wt}/**)"),
        format!("Glob({wt}/**)"),
        format!("Grep({wt}/**)"),
        "Bash(git add:*)".into(),
        "Bash(git commit:*)".into(),
        "Bash(git diff:*)".into(),
        "Bash(git log:*)".into(),
        "Bash(git stash:*)".into(),
        "Bash(git status:*)".into(),
    ];
    let deny = vec![
        "Bash(git push*)".into(),
        "Bash(git checkout main*)".into(),
        "Bash(git checkout master*)".into(),
    ];
    (allow, deny)
}

/// Merge foundry hooks into an existing hooks object. Appends foundry hook
/// entries to each event's array without removing existing hooks.
fn merge_hooks(
    existing: &serde_json::Value,
    foundry_hooks: &serde_json::Value,
) -> serde_json::Value {
    let mut merged = existing.clone();

    if let (Some(merged_obj), Some(foundry_obj)) =
        (merged.as_object_mut(), foundry_hooks.as_object())
    {
        for (event, foundry_entries) in foundry_obj {
            if let Some(foundry_arr) = foundry_entries.as_array() {
                let existing_arr = merged_obj
                    .entry(event.clone())
                    .or_insert_with(|| serde_json::json!([]));
                if let Some(arr) = existing_arr.as_array_mut() {
                    arr.extend(foundry_arr.clone());
                }
            }
        }
    }

    merged
}

/// Merge permission arrays, appending new entries and optionally stripping
/// patterns that match any deny rule (used to remove git push from copied allows).
fn merge_permissions(
    existing: &[serde_json::Value],
    additions: &[String],
    strip_patterns: &[&str],
) -> Vec<serde_json::Value> {
    let mut result: Vec<serde_json::Value> = existing
        .iter()
        .filter(|v| {
            if let Some(s) = v.as_str() {
                !strip_patterns.iter().any(|pat| s.contains(pat))
            } else {
                true
            }
        })
        .cloned()
        .collect();

    for addition in additions {
        let val = serde_json::Value::String(addition.clone());
        if !result.contains(&val) {
            result.push(val);
        }
    }

    result
}

/// Set up agent-specific workspace configuration. For Claude, creates
/// .claude/settings.local.json with status hooks and worktree-scoped
/// permissions. For other agents, performs any applicable setup.
/// If the source repo has existing agent config, it is used as the base.
pub fn setup_agent_hooks(
    worktree_path: &Path,
    source_path: &Path,
    project: &str,
    name: &str,
    agent: &str,
) -> Result<()> {
    match agent {
        "claude" => setup_claude(worktree_path, source_path, project, name),
        // Codex permissions are handled via CLI flags in resolve_agent_command.
        // No config file setup needed since .codex/config.toml is tracked in git.
        _ => Ok(()),
    }
}

/// Claude-specific setup: create .claude/settings.local.json with status
/// tracking hooks and worktree-scoped permissions.
fn setup_claude(worktree_path: &Path, source_path: &Path, project: &str, name: &str) -> Result<()> {
    let status_path = status_file_path(project, name, "claude")?;
    let status_path_str = status_path.to_string_lossy();

    // Ensure the status directory exists
    if let Some(parent) = status_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create status directory {}", parent.display()))?;
    }

    // Write initial status
    std::fs::write(&status_path, "idle")
        .with_context(|| format!("failed to write status file {}", status_path.display()))?;

    let claude_dir = worktree_path.join(".claude");
    std::fs::create_dir_all(&claude_dir).with_context(|| {
        format!(
            "failed to create .claude directory in {}",
            worktree_path.display()
        )
    })?;

    // Load existing settings.local.json from source repo as base
    let source_settings_path = source_path.join(".claude").join("settings.local.json");
    let mut settings: serde_json::Value = if source_settings_path.exists() {
        let content = std::fs::read_to_string(&source_settings_path)
            .with_context(|| format!("failed to read {}", source_settings_path.display()))?;
        serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    // Merge hooks
    let foundry_hooks = build_status_hooks(&status_path_str);
    let existing_hooks = settings
        .get("hooks")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let merged_hooks = merge_hooks(&existing_hooks, &foundry_hooks);
    settings["hooks"] = merged_hooks;

    // Merge permissions
    let (worktree_allow, worktree_deny) = build_worktree_permissions(worktree_path);

    let existing_allow = settings
        .get("permissions")
        .and_then(|p| p.get("allow"))
        .and_then(|a| a.as_array())
        .cloned()
        .unwrap_or_default();

    let existing_deny = settings
        .get("permissions")
        .and_then(|p| p.get("deny"))
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default();

    // Strip git push from any copied allow rules
    let merged_allow = merge_permissions(&existing_allow, &worktree_allow, &["git push"]);
    let merged_deny = merge_permissions(&existing_deny, &worktree_deny, &[]);

    // Ensure permissions object exists
    if settings.get("permissions").is_none() {
        settings["permissions"] = serde_json::json!({});
    }
    settings["permissions"]["allow"] = serde_json::Value::Array(merged_allow);
    settings["permissions"]["deny"] = serde_json::Value::Array(merged_deny);

    let settings_path = claude_dir.join("settings.local.json");
    let contents =
        serde_json::to_string_pretty(&settings).context("failed to serialize settings")?;

    std::fs::write(&settings_path, contents)
        .with_context(|| format!("failed to write {}", settings_path.display()))?;

    Ok(())
}
