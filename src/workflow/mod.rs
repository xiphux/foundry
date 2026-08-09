pub mod checks;
pub mod cleanup;
pub mod diff;
pub mod discard;
pub mod edit;
pub mod finish;
pub mod open;
pub mod pr;
pub mod restore;
pub mod scripts;
pub mod start;
pub mod status;

pub use cleanup::{BranchCleanup, cleanup_workspace};
pub use scripts::{ScriptKind, run_scripts};

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

use crate::config;
use crate::git;
use crate::registry::Registry;

pub fn resolve_project(
    project_flag: Option<&str>,
    registry: &mut Registry,
    registry_path: &Path,
) -> Result<(String, PathBuf)> {
    if let Some(name) = project_flag {
        let path = registry.get(name).with_context(|| {
            format!("project '{name}' not found. Register it with `foundry projects add`.")
        })?;
        return Ok((name.to_string(), path));
    }

    let cwd = std::env::current_dir().context("failed to get current directory")?;

    // `main_repo_root`, not `repo_root`: inside a linked worktree the latter
    // reports the *worktree*, which is not a project. Running `foundry start`
    // from a workspace tab therefore auto-registered that workspace's worktree
    // as a brand-new project, and then created worktrees of it — nesting
    // workspaces inside workspaces, with a registry entry that dangles as soon
    // as the original workspace is finished. Resolving back to the source repo
    // is the same normalization the trust gate does, for the same reason.
    let repo_root = git::main_repo_root(&cwd).context("not inside a git repository")?;

    if let Some(name) = registry.find_by_path(&repo_root) {
        return Ok((name, repo_root));
    }

    let name = repo_root
        .file_name()
        .context("repo root has no directory name")?
        .to_str()
        .context("directory name is not valid UTF-8")?
        .to_string();

    if registry.get(&name).is_some() {
        anyhow::bail!(
            "project name '{name}' is already registered to a different path. \
             Use `foundry projects add <custom-name> {}` to register with a different name.",
            repo_root.display()
        );
    }

    eprintln!(
        "Auto-registering project '{name}' at {}",
        repo_root.display()
    );
    registry.add(&name, repo_root.clone())?;
    registry.save_to(registry_path)?;

    Ok((name, repo_root))
}

/// An active workspace, resolved from recorded state rather than rebuilt.
///
/// Every command that operates on an existing workspace needs the same handful
/// of facts, and each used to derive them itself:
///
/// ```ignore
/// let worktree_path = config.worktree_dir.join(project_name).join(name);
/// if !worktree_path.exists() { bail!("worktree '{name}' does not exist") }
/// let workspace = state.find_by_worktree_path(&worktree_path.to_string_lossy())
///     .ok_or_else(|| anyhow!("workspace '{name}' not found in state"))?;
/// ```
///
/// That recomputes the worktree path from `worktree_dir` — but `state.toml`
/// already records where the worktree actually is. The two agree only while
/// the config is unchanged, so editing `worktree_dir` made every existing
/// workspace unreachable: each command rebuilt a path that was never created,
/// found nothing there, and reported "worktree does not exist" about a
/// worktree sitting healthy at its recorded location. Including `discard`, so
/// there was no way back short of hand-editing state.
///
/// Reading the recorded path instead makes `worktree_dir` mean what it says —
/// where *new* worktrees go — and leaves existing ones alone.
#[derive(Debug, Clone)]
pub struct ActiveWorkspace {
    pub name: String,
    pub project: String,
    pub source_path: PathBuf,
    pub worktree_path: PathBuf,
    pub branch: String,
    pub terminal_tab_id: String,
    pub pr_number: Option<u64>,
    pub pr_url: Option<String>,
}

/// Resolve an active workspace, or explain which half is missing.
///
/// The two failures are worth separating. Absent from state means foundry is
/// not tracking it; present in state but gone from disk means something removed
/// the directory behind foundry's back, and the state entry needs clearing.
pub fn resolve_active_workspace(
    state: &crate::state::WorkspaceState,
    project: &str,
    name: &str,
) -> Result<ActiveWorkspace> {
    let ws = state.find(project, name).ok_or_else(|| {
        anyhow::anyhow!(
            "workspace '{name}' is not active in project '{project}'. \
             Run `foundry list` to see active workspaces."
        )
    })?;

    let worktree_path = PathBuf::from(&ws.worktree_path);
    if !worktree_path.exists() {
        bail!(
            "workspace '{name}' is recorded at {} but that directory no longer exists. \
             Run `foundry list` to refresh, or `foundry discard {name}` to clear the entry.",
            worktree_path.display()
        );
    }

    Ok(ActiveWorkspace {
        name: ws.name.clone(),
        project: ws.project.clone(),
        source_path: PathBuf::from(&ws.source_path),
        worktree_path,
        branch: ws.branch.clone(),
        terminal_tab_id: ws.terminal_tab_id.clone(),
        pr_number: ws.pr_number,
        pr_url: ws.pr_url.clone(),
    })
}

/// Longest workspace name accepted. The name is one component of a path that
/// also carries the worktree root and the project name, so this leaves room
/// under the usual 255-byte limit on a single filesystem component.
const MAX_WORKSPACE_NAME_LEN: usize = 100;

/// Reject workspace names that are unsafe to use as a path component.
///
/// The name is not just a label: it is joined into the worktree path, used as
/// a git branch name, embedded in the status filename, and interpolated into
/// the quoted hook command written to `.claude/settings.local.json`. A `/` or
/// a `..` therefore escapes the worktree root, and a `'` breaks out of the
/// quoting in the hook command.
///
/// Unicode letters and digits are allowed, because `--issue` derives the name
/// from the issue title and titles are not always Latin script. Everything
/// outside letters, digits, `-`, `_` and `.` is refused.
pub fn validate_workspace_name(name: &str) -> Result<()> {
    if name.is_empty() {
        bail!("workspace name cannot be empty");
    }
    if name.len() > MAX_WORKSPACE_NAME_LEN {
        bail!(
            "workspace name is too long ({} bytes, maximum {MAX_WORKSPACE_NAME_LEN})",
            name.len()
        );
    }
    // A leading '-' reads as a flag to git; a leading '.' hides the worktree
    // and covers the `.` and `..` path components.
    if name.starts_with('-') {
        bail!("workspace name cannot start with '-': {name:?}");
    }
    if name.starts_with('.') {
        bail!("workspace name cannot start with '.': {name:?}");
    }
    if let Some(bad) = name
        .chars()
        .find(|c| !(c.is_alphanumeric() || *c == '-' || *c == '_' || *c == '.'))
    {
        bail!(
            "workspace name contains an unsupported character {bad:?}: {name:?}. \
             Use letters, digits, '-', '_' or '.'."
        );
    }
    Ok(())
}

pub fn compute_branch_name(name: &str, prefix: Option<&str>) -> String {
    match prefix {
        Some(p) if !p.is_empty() => format!("{p}/{name}"),
        _ => name.to_string(),
    }
}

/// Ending port for dynamic allocation range (exclusive).
const PORT_RANGE_END: u16 = 60000;

/// Allocate a contiguous block of ports for a new workspace.
/// Scans from `range_start` to find a contiguous block of `port_names.len()`
/// ports that don't overlap with any already-reserved ports.
pub fn allocate_ports(
    port_names: &[String],
    reserved: &[u16],
    range_start: u16,
) -> std::collections::HashMap<String, u16> {
    let count = port_names.len();
    if count == 0 {
        return std::collections::HashMap::new();
    }

    let mut sorted_reserved: Vec<u16> = reserved.to_vec();
    sorted_reserved.sort();

    // Find the first contiguous block of `count` ports in the range
    let mut start = range_start;
    'outer: while start + count as u16 <= PORT_RANGE_END {
        for offset in 0..count as u16 {
            let port = start + offset;
            if sorted_reserved.binary_search(&port).is_ok() {
                // This port is taken — skip past it
                start = port + 1;
                continue 'outer;
            }
        }
        // Found a contiguous block
        break;
    }

    port_names
        .iter()
        .enumerate()
        .map(|(i, name)| (name.clone(), start + i as u16))
        .collect()
}

pub fn foundry_paths() -> Result<(PathBuf, PathBuf)> {
    let foundry_dir = config::foundry_dir()?;
    Ok((
        foundry_dir.join("projects.toml"),
        foundry_dir.join("state.toml"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_workspace_name_accepts_ordinary_names() {
        for name in ["feature", "fix-auth-timeout", "v1.2_beta", "42-fix-auth"] {
            assert!(
                validate_workspace_name(name).is_ok(),
                "should accept {name:?}"
            );
        }
    }

    /// `--issue` derives the name from the issue title, which is not always
    /// Latin script, so letters outside ASCII have to stay valid.
    #[test]
    fn validate_workspace_name_accepts_non_ascii_letters() {
        assert!(validate_workspace_name("42-日本語").is_ok());
        assert!(validate_workspace_name("café").is_ok());
    }

    /// The name is joined into the worktree path, so anything that could climb
    /// out of it has to be refused.
    #[test]
    fn validate_workspace_name_rejects_path_traversal() {
        for name in ["..", ".", "../evil", "a/b", "a\\b", ".hidden", "/abs"] {
            assert!(
                validate_workspace_name(name).is_err(),
                "should reject {name:?}"
            );
        }
    }

    /// The name is interpolated into the single-quoted hook command written to
    /// settings.local.json, so a quote must not get through.
    #[test]
    fn validate_workspace_name_rejects_shell_metacharacters() {
        for name in [
            "a'b",
            "a\"b",
            "a;rm -rf /",
            "a$(id)",
            "a`id`",
            "a|b",
            "a b",
            "a\nb",
        ] {
            assert!(
                validate_workspace_name(name).is_err(),
                "should reject {name:?}"
            );
        }
    }

    /// A leading hyphen would be read as a flag by git.
    #[test]
    fn validate_workspace_name_rejects_leading_hyphen() {
        assert!(validate_workspace_name("-force").is_err());
        assert!(validate_workspace_name("--upload-pack=evil").is_err());
    }

    #[test]
    fn validate_workspace_name_rejects_empty_and_overlong() {
        assert!(validate_workspace_name("").is_err());
        assert!(validate_workspace_name(&"a".repeat(MAX_WORKSPACE_NAME_LEN)).is_ok());
        assert!(validate_workspace_name(&"a".repeat(MAX_WORKSPACE_NAME_LEN + 1)).is_err());
    }

    #[test]
    fn compute_branch_name_with_prefix() {
        assert_eq!(
            compute_branch_name("my-feature", Some("user")),
            "user/my-feature"
        );
    }

    #[test]
    fn compute_branch_name_without_prefix() {
        assert_eq!(compute_branch_name("my-feature", None), "my-feature");
    }

    #[test]
    fn compute_branch_name_with_empty_prefix() {
        assert_eq!(compute_branch_name("my-feature", Some("")), "my-feature");
    }

    #[test]
    fn allocate_ports_contiguous_block() {
        let names = vec!["VITE_PORT".into(), "API_PORT".into(), "DB_PORT".into()];
        let ports = allocate_ports(&names, &[], 10000);
        assert_eq!(ports["VITE_PORT"], 10000);
        assert_eq!(ports["API_PORT"], 10001);
        assert_eq!(ports["DB_PORT"], 10002);
    }

    #[test]
    fn allocate_ports_skips_reserved() {
        let names = vec!["PORT_A".into()];
        let ports = allocate_ports(&names, &[10000], 10000);
        assert_eq!(ports["PORT_A"], 10001);
    }

    #[test]
    fn allocate_ports_finds_gap_after_reserved_block() {
        let names = vec!["PORT_A".into(), "PORT_B".into()];
        let ports = allocate_ports(&names, &[10000], 10000);
        assert_eq!(ports["PORT_A"], 10001);
        assert_eq!(ports["PORT_B"], 10002);
    }

    #[test]
    fn allocate_ports_skips_fragmented_reserved() {
        let names = vec!["PORT_A".into(), "PORT_B".into(), "PORT_C".into()];
        let reserved = vec![10000, 10002];
        let ports = allocate_ports(&names, &reserved, 10000);
        assert_eq!(ports["PORT_A"], 10003);
        assert_eq!(ports["PORT_B"], 10004);
        assert_eq!(ports["PORT_C"], 10005);
    }

    #[test]
    fn allocate_ports_empty_names() {
        let ports = allocate_ports(&[], &[], 10000);
        assert!(ports.is_empty());
    }

    #[test]
    fn allocate_ports_custom_range_start() {
        let names = vec!["PORT_A".into()];
        let ports = allocate_ports(&names, &[], 20000);
        assert_eq!(ports["PORT_A"], 20000);
    }
}
