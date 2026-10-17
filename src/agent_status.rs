//! Per-workspace agent status: what each agent's hooks report, and where.
//!
//! The hook script installed by `agent_hooks` writes one JSON file per agent
//! per workspace under `~/.foundry/status/<project>/<name>/`; `foundry status`
//! reads them and cleanup removes them. Also here: the per-workspace context
//! file, the other runtime file cleanup removes.

use anyhow::Result;
use std::path::PathBuf;

use crate::config;

/// The possible agent statuses we track.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AgentStatus {
    Working,
    Idle,
    WaitingPermission,
    Error,
    Offline,
    Unknown,
}

/// Rich status information parsed from a JSON status file.
#[derive(Debug, Clone)]
pub struct AgentStatusInfo {
    pub status: AgentStatus,
    pub last_tool: Option<String>,
    pub last_message: Option<String>,
    pub error: Option<String>,
    /// Millisecond timestamp of the last status update (from Date.now() in JS).
    pub updated_at: Option<u64>,
}

impl Default for AgentStatusInfo {
    fn default() -> Self {
        Self {
            status: AgentStatus::Unknown,
            last_tool: None,
            last_message: None,
            error: None,
            updated_at: None,
        }
    }
}

/// How long a "working" status can go without an update before we consider it stale.
const STALE_WORKING_THRESHOLD_MS: u64 = 300_000; // 5 minutes

impl AgentStatusInfo {
    /// Returns true if the status is "working" but hasn't been updated recently,
    /// suggesting the agent was interrupted or stalled without a proper Stop event.
    pub fn is_stale(&self) -> bool {
        if self.status != AgentStatus::Working {
            return false;
        }
        if let Some(updated_at) = self.updated_at {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            now_ms.saturating_sub(updated_at) > STALE_WORKING_THRESHOLD_MS
        } else {
            false // no timestamp means old format, can't determine staleness
        }
    }
}

/// The directory holding one workspace's agent status files.
///
/// Each workspace owns a directory rather than sharing one with its
/// neighbours. The flat `{name}-{agent}.json` layout this replaced was
/// ambiguous, because workspace names may contain `-`: given workspaces `foo`
/// and `foo-bar`, the file `foo-bar-claude.json` parses as agent `bar-claude`
/// of `foo` exactly as well as agent `claude` of `foo-bar`. The dashboard
/// invented the phantom agent, and — the damaging half — `remove_status("foo")`
/// matched the same file and deleted the *neighbour's* live status on every
/// finish and discard.
fn status_dir(project: &str, name: &str) -> Result<PathBuf> {
    Ok(config::foundry_dir()?
        .join("status")
        .join(project)
        .join(name))
}

/// Get the path to the status file for a specific agent in a workspace.
pub(crate) fn status_file_path(project: &str, name: &str, agent: &str) -> Result<PathBuf> {
    Ok(status_dir(project, name)?.join(format!("{agent}.json")))
}

/// The flat path a workspace created before the per-workspace directory still
/// writes to.
///
/// The hook command is baked into each worktree's `.claude/settings.local.json`
/// when the workspace is created, so a workspace that predates this layout
/// keeps writing the old path for the rest of its life. Reading it keeps those
/// workspaces on the dashboard instead of showing "unknown" until they are
/// finished. Nothing writes this path any more.
///
/// Can be deleted once no workspace created before the change can still be open.
///
/// `.status` was an even older spelling than `.json`; both are read, neither is
/// written, so both are returned here.
fn legacy_status_file_paths(project: &str, name: &str, agent: &str) -> Vec<PathBuf> {
    let Ok(dir) = config::foundry_dir().map(|d| d.join("status").join(project)) else {
        return Vec::new();
    };
    vec![
        dir.join(format!("{name}-{agent}.json")),
        dir.join(format!("{name}-{agent}.status")),
    ]
}

/// Agent ids that a legacy flat filename can be read as for this workspace.
///
/// Only a *known* agent id is accepted, and that is what makes the ambiguous
/// case safe rather than merely less likely: for workspace `foo`, the file
/// `foo-bar-claude.json` yields `bar-claude`, which is not a registered agent,
/// so it is skipped — and left to its real owner, workspace `foo-bar`.
fn legacy_agents(project: &str, name: &str) -> Vec<String> {
    let Ok(dir) = config::foundry_dir().map(|d| d.join("status").join(project)) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };

    let prefix = format!("{name}-");
    let mut agents = Vec::new();
    for entry in entries.flatten() {
        let filename = entry.file_name().to_string_lossy().to_string();
        let Some(rest) = filename.strip_prefix(&prefix) else {
            continue;
        };
        let Some(agent) = rest
            .strip_suffix(".json")
            .or_else(|| rest.strip_suffix(".status"))
        else {
            continue;
        };
        if !agent.is_empty()
            && crate::config::agent_capabilities(agent).is_some()
            && !agents.iter().any(|a| a == agent)
        {
            agents.push(agent.to_string());
        }
    }
    agents
}

/// Map the `status` field written by the hook script onto the enum.
fn parse_status(raw: &str) -> AgentStatus {
    match raw {
        "working" => AgentStatus::Working,
        "idle" => AgentStatus::Idle,
        "waiting_permission" => AgentStatus::WaitingPermission,
        "error" => AgentStatus::Error,
        "offline" => AgentStatus::Offline,
        _ => AgentStatus::Unknown,
    }
}

/// Read rich status info (JSON with metadata) for a specific agent in a workspace.
/// Falls back to plain text for backwards compatibility.
pub fn read_status_info(project: &str, name: &str, agent: &str) -> AgentStatusInfo {
    let content = status_file_path(project, name, agent)
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .or_else(|| {
            legacy_status_file_paths(project, name, agent)
                .into_iter()
                .find_map(|p| std::fs::read_to_string(p).ok())
        });

    let Some(content) = content else {
        return AgentStatusInfo::default();
    };
    let trimmed = content.trim();

    // Try JSON parse first
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) {
        return AgentStatusInfo {
            status: parse_status(json.get("status").and_then(|v| v.as_str()).unwrap_or("")),
            last_tool: json
                .get("last_tool")
                .and_then(|v| v.as_str())
                .map(String::from),
            last_message: json
                .get("last_message")
                .and_then(|v| v.as_str())
                .map(String::from),
            error: json.get("error").and_then(|v| v.as_str()).map(String::from),
            updated_at: json.get("updated_at").and_then(|v| v.as_u64()),
        };
    }

    // Backwards compatibility: plain text format
    AgentStatusInfo {
        status: parse_status(trimmed),
        ..Default::default()
    }
}

/// Read rich status infos for all agents in a workspace. Returns a list of
/// (agent_name, AgentStatusInfo) pairs, sorted by agent name.
pub fn read_all_status_infos(project: &str, name: &str) -> Vec<(String, AgentStatusInfo)> {
    let mut agents: Vec<String> = status_dir(project, name)
        .and_then(|d| Ok(std::fs::read_dir(d)?))
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|e| {
                    let filename = e.file_name().to_string_lossy().to_string();
                    filename.strip_suffix(".json").map(String::from)
                })
                .filter(|agent| !agent.is_empty())
                .collect()
        })
        .unwrap_or_default();

    for agent in legacy_agents(project, name) {
        if !agents.contains(&agent) {
            agents.push(agent);
        }
    }

    agents.sort();
    agents
        .into_iter()
        .map(|agent| {
            let info = read_status_info(project, name, &agent);
            (agent, info)
        })
        .collect()
}

/// Remove all status files for a workspace (cleanup on finish/discard).
pub fn remove_status(project: &str, name: &str) {
    if let Ok(dir) = status_dir(project, name) {
        let _ = std::fs::remove_dir_all(dir);
    }

    // Legacy flat files, matched only where the agent id is unambiguous — see
    // `legacy_agents`. This is the path that used to delete a neighbouring
    // workspace's status.
    for agent in legacy_agents(project, name) {
        for path in legacy_status_file_paths(project, name, &agent) {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Remove the context file for a workspace (cleanup on finish/discard).
pub fn remove_context(project: &str, name: &str) {
    let foundry_dir = match config::foundry_dir() {
        Ok(d) => d,
        Err(_) => return,
    };

    let context_path = foundry_dir
        .join("context")
        .join(project)
        .join(format!("{name}.txt"));
    let _ = std::fs::remove_file(context_path);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write a status file in the current per-workspace layout.
    fn write_status(project: &str, name: &str, agent: &str, body: &str) -> PathBuf {
        let path = status_file_path(project, name, agent).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, body).unwrap();
        path
    }

    /// Write a status file in the flat layout used before the per-workspace
    /// directory, as a workspace created by an older foundry still would.
    fn write_legacy_status(project: &str, name: &str, agent: &str, body: &str) -> PathBuf {
        let path = legacy_status_file_paths(project, name, agent)
            .into_iter()
            .next()
            .unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, body).unwrap();
        path
    }

    fn status_json(status: &str) -> String {
        format!(r#"{{"status":"{status}","last_tool":null,"last_message":null,"error":null}}"#)
    }

    #[test]
    fn status_file_path_gives_each_workspace_its_own_directory() {
        let path = status_file_path("myproject", "my-workspace", "claude").unwrap();
        assert!(path.ends_with("status/myproject/my-workspace/claude.json"));
        assert!(path.to_string_lossy().contains(".foundry"));
    }

    #[test]
    fn read_status_info_parses_each_status() {
        for (raw, expected) in [
            ("working", AgentStatus::Working),
            ("idle", AgentStatus::Idle),
            ("waiting_permission", AgentStatus::WaitingPermission),
            ("error", AgentStatus::Error),
            ("offline", AgentStatus::Offline),
        ] {
            let project = format!("testproj_parse_{raw}");
            write_status(&project, "testws", "claude", &status_json(raw));
            assert_eq!(
                read_status_info(&project, "testws", "claude").status,
                expected,
                "for {raw}"
            );
            remove_status(&project, "testws");
        }
    }

    #[test]
    fn read_status_info_missing_file() {
        assert_eq!(
            read_status_info("nonexistent_proj_xyz2", "nonexistent_ws", "claude").status,
            AgentStatus::Unknown
        );
    }

    #[test]
    fn read_status_info_invalid_content() {
        write_status("testproj_invalid2", "testws", "claude", "bogus_value");
        assert_eq!(
            read_status_info("testproj_invalid2", "testws", "claude").status,
            AgentStatus::Unknown
        );
        remove_status("testproj_invalid2", "testws");
    }

    #[test]
    fn remove_status_existing_file() {
        let path = write_status("testproj_rm2", "testws", "claude", &status_json("idle"));
        assert!(path.exists());
        remove_status("testproj_rm2", "testws");
        assert!(!path.exists());
    }

    /// Workspace names may contain `-`, so the flat `{name}-{agent}.json`
    /// layout could not tell agent `bar-claude` of `foo` from agent `claude` of
    /// `foo-bar`. Finishing `foo` deleted `foo-bar`'s live status.
    #[test]
    fn removing_a_workspace_leaves_a_similarly_named_neighbour_alone() {
        let neighbour = write_status("testproj_nbr", "foo-bar", "claude", &status_json("working"));
        write_status("testproj_nbr", "foo", "claude", &status_json("idle"));

        remove_status("testproj_nbr", "foo");

        assert!(neighbour.exists(), "neighbour's status was deleted");
        assert_eq!(
            read_status_info("testproj_nbr", "foo-bar", "claude").status,
            AgentStatus::Working
        );
        remove_status("testproj_nbr", "foo-bar");
    }

    /// The same ambiguity on the read side: `foo` must not report an agent
    /// invented out of its neighbour's filename.
    #[test]
    fn a_workspace_does_not_report_a_neighbours_agent() {
        write_status("testproj_ph", "foo-bar", "claude", &status_json("working"));
        write_status("testproj_ph", "foo", "claude", &status_json("idle"));

        let agents: Vec<String> = read_all_status_infos("testproj_ph", "foo")
            .into_iter()
            .map(|(a, _)| a)
            .collect();
        assert_eq!(agents, ["claude"]);

        remove_status("testproj_ph", "foo");
        remove_status("testproj_ph", "foo-bar");
    }

    /// A workspace created before the layout change keeps writing the flat path
    /// for its whole life, so it has to stay readable.
    #[test]
    fn legacy_flat_status_is_still_read() {
        let path =
            write_legacy_status("testproj_lgcy", "testws", "claude", &status_json("working"));

        let info = read_status_info("testproj_lgcy", "testws", "claude");
        assert_eq!(info.status, AgentStatus::Working);

        let agents: Vec<String> = read_all_status_infos("testproj_lgcy", "testws")
            .into_iter()
            .map(|(a, _)| a)
            .collect();
        assert_eq!(agents, ["claude"]);

        remove_status("testproj_lgcy", "testws");
        assert!(!path.exists(), "legacy file should be cleaned up too");
    }

    /// The legacy reader accepts a filename only when the trailing segment is a
    /// registered agent — that is what keeps the ambiguous case safe rather than
    /// merely unlikely.
    #[test]
    fn legacy_reader_ignores_a_filename_that_is_really_a_neighbour() {
        let neighbour = write_legacy_status(
            "testproj_lgcy2",
            "foo-bar",
            "claude",
            &status_json("working"),
        );

        assert!(legacy_agents("testproj_lgcy2", "foo").is_empty());
        remove_status("testproj_lgcy2", "foo");
        assert!(neighbour.exists(), "neighbour's legacy status was deleted");

        remove_status("testproj_lgcy2", "foo-bar");
    }

    #[test]
    fn remove_status_nonexistent_file() {
        // Should not panic
        remove_status("nonexistent_proj_rm_xyz2", "nonexistent_ws");
    }

    #[test]
    fn read_status_json_working_with_tool() {
        let path = status_file_path("testproj_json1", "testws", "claude").unwrap();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(
            &path,
            r#"{"status":"working","last_tool":"Edit auth.rs","last_message":null,"error":null}"#,
        )
        .unwrap();
        let info = read_status_info("testproj_json1", "testws", "claude");
        assert_eq!(info.status, AgentStatus::Working);
        assert_eq!(info.last_tool.as_deref(), Some("Edit auth.rs"));
        assert!(info.last_message.is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn read_status_json_idle_with_message() {
        let path = status_file_path("testproj_json2", "testws", "claude").unwrap();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(
            &path,
            r#"{"status":"idle","last_tool":null,"last_message":"Done refactoring","error":null}"#,
        )
        .unwrap();
        let info = read_status_info("testproj_json2", "testws", "claude");
        assert_eq!(info.status, AgentStatus::Idle);
        assert_eq!(info.last_message.as_deref(), Some("Done refactoring"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn read_status_json_error() {
        let path = status_file_path("testproj_json3", "testws", "claude").unwrap();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(
            &path,
            r#"{"status":"error","last_tool":null,"last_message":null,"error":"rate_limit"}"#,
        )
        .unwrap();
        let info = read_status_info("testproj_json3", "testws", "claude");
        assert_eq!(info.status, AgentStatus::Error);
        assert_eq!(info.error.as_deref(), Some("rate_limit"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn read_status_json_offline() {
        let path = status_file_path("testproj_json4", "testws", "claude").unwrap();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(
            &path,
            r#"{"status":"offline","last_tool":null,"last_message":null,"error":null}"#,
        )
        .unwrap();
        let info = read_status_info("testproj_json4", "testws", "claude");
        assert_eq!(info.status, AgentStatus::Offline);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn read_status_json_missing_file_returns_unknown() {
        let info = read_status_info("nonexistent_json_proj", "testws", "claude");
        assert_eq!(info.status, AgentStatus::Unknown);
    }

    #[test]
    fn read_status_json_backwards_compat_plain_text() {
        let path = status_file_path("testproj_compat", "testws", "claude").unwrap();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, "working").unwrap();
        let info = read_status_info("testproj_compat", "testws", "claude");
        assert_eq!(info.status, AgentStatus::Working);
        let _ = std::fs::remove_file(&path);
    }
}
