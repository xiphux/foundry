use anyhow::{Context, Result};
use std::process::Command;

pub fn run(ctx: &mut super::WorkflowCtx, name: &str) -> Result<()> {
    let worktree_path = ctx.workspace(name)?.worktree_path;

    let editor = resolve_editor(ctx.config.editor.as_deref())?;

    if ctx.verbose {
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

pub fn browse(ctx: &mut super::WorkflowCtx, name: &str) -> Result<()> {
    let worktree_path = ctx.workspace(name)?.worktree_path;

    let cmd = file_explorer_command();

    if ctx.verbose {
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
