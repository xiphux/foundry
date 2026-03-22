pub mod discard;
pub mod finish;
pub mod open;
pub mod restore;
pub mod start;
pub mod status;

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

pub fn foundry_paths() -> Result<(PathBuf, PathBuf)> {
    let foundry_dir = config::foundry_dir()?;
    Ok((
        foundry_dir.join("projects.toml"),
        foundry_dir.join("state.toml"),
    ))
}
