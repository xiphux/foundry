use anyhow::{Context, Result};
use std::process::Command;

use crate::config::ResolvedConfig;
use crate::state::WorkspaceState;

pub fn run(
    name: &str,
    project_name: &str,
    config: &ResolvedConfig,
    state: &WorkspaceState,
    verbose: bool,
) -> Result<()> {
    let worktree_path = config.worktree_dir.join(project_name).join(name);

    if !worktree_path.exists() {
        anyhow::bail!(
            "worktree '{name}' does not exist at {}",
            worktree_path.display()
        );
    }

    // Verify workspace is tracked in state
    state
        .find_by_worktree_path(&worktree_path.to_string_lossy())
        .ok_or_else(|| anyhow::anyhow!("workspace '{name}' not found in state"))?;

    let editor = resolve_editor(config.editor.as_deref())?;

    if verbose {
        eprintln!("Opening {} in '{editor}'...", worktree_path.display());
    }

    Command::new(&editor)
        .arg(&worktree_path)
        .spawn()
        .with_context(|| format!("failed to launch editor '{editor}'"))?;

    Ok(())
}

fn resolve_editor(configured: Option<&str>) -> Result<String> {
    if let Some(editor) = configured {
        return Ok(editor.to_string());
    }

    if let Ok(visual) = std::env::var("VISUAL")
        && !visual.is_empty()
    {
        return Ok(visual);
    }

    if let Ok(editor) = std::env::var("EDITOR")
        && !editor.is_empty()
    {
        return Ok(editor);
    }

    anyhow::bail!(
        "no editor configured. Set `editor` in ~/.foundry/config.toml, \
         or set the VISUAL or EDITOR environment variable."
    )
}

pub fn browse(
    name: &str,
    project_name: &str,
    config: &ResolvedConfig,
    state: &WorkspaceState,
    verbose: bool,
) -> Result<()> {
    let worktree_path = config.worktree_dir.join(project_name).join(name);

    if !worktree_path.exists() {
        anyhow::bail!(
            "worktree '{name}' does not exist at {}",
            worktree_path.display()
        );
    }

    state
        .find_by_worktree_path(&worktree_path.to_string_lossy())
        .ok_or_else(|| anyhow::anyhow!("workspace '{name}' not found in state"))?;

    let cmd = file_explorer_command();

    if verbose {
        eprintln!("Opening {} in file explorer...", worktree_path.display());
    }

    Command::new(cmd)
        .arg(&worktree_path)
        .spawn()
        .with_context(|| format!("failed to launch file explorer '{cmd}'"))?;

    Ok(())
}

fn file_explorer_command() -> &'static str {
    if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_editor_configured() {
        assert_eq!(resolve_editor(Some("code")).unwrap(), "code");
    }

    #[test]
    fn resolve_editor_configured_takes_precedence_over_env() {
        // Configured value always wins, regardless of env vars
        assert_eq!(resolve_editor(Some("cursor")).unwrap(), "cursor");
    }

    #[test]
    fn file_explorer_is_known_command() {
        let cmd = file_explorer_command();
        assert!(["open", "explorer", "xdg-open"].contains(&cmd));
    }
}
