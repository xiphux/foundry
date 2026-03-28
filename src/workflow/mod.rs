pub mod checks;
pub mod cleanup;
pub mod diff;
pub mod discard;
pub mod edit;
pub mod finish;
pub mod open;
pub mod pr;
pub mod restore;
pub mod start;
pub mod status;

pub use cleanup::{BranchCleanup, cleanup_workspace};

use anyhow::{Context, Result};
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
    let repo_root = git::repo_root(&cwd).context("not inside a git repository")?;

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
