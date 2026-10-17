use anyhow::{Context, Result};
use std::process::Command;

pub fn run(ctx: &mut super::WorkflowCtx, name: &str) -> Result<()> {
    let worktree_path = ctx.workspace(name)?.worktree_path;

    let editor = resolve_editor(ctx.config.editor.as_deref())?;

    if ctx.verbose {
        eprintln!("Opening {} in '{editor}'...", worktree_path.display());
    }

    let (program, args) = split_editor(&editor);
    Command::new(program)
        .args(args)
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

/// Split an editor setting into program and leading arguments.
///
/// `EDITOR="code --wait"` and `emacsclient -n` are the convention, and a whole
/// string handed to `Command::new` names a program that does not exist. Git
/// gets this by running the value through `sh`, which Windows lacks, so this
/// splits on whitespace instead — unless the whole value is an existing file,
/// which keeps a path with spaces in it working.
fn split_editor(editor: &str) -> (&str, Vec<&str>) {
    if std::path::Path::new(editor).is_file() {
        return (editor, Vec::new());
    }
    let mut parts = editor.split_whitespace();
    let program = parts.next().unwrap_or(editor);
    (program, parts.collect())
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
    fn split_editor_separates_arguments() {
        assert_eq!(split_editor("code --wait"), ("code", vec!["--wait"]));
        assert_eq!(split_editor("  vim  "), ("vim", vec![]));
    }

    #[test]
    fn split_editor_keeps_an_existing_path_with_spaces_whole() {
        let dir = tempfile::TempDir::new().unwrap();
        let editor = dir.path().join("My Editor");
        std::fs::write(&editor, "").unwrap();
        let editor = editor.to_str().unwrap();
        assert_eq!(split_editor(editor), (editor, vec![]));
    }

    #[test]
    fn file_explorer_is_known_command() {
        let cmd = file_explorer_command();
        assert!(["open", "explorer", "xdg-open"].contains(&cmd));
    }
}
