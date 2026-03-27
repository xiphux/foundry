use anyhow::{Result, bail};
use std::path::Path;
use std::process::Command;

use super::{PaneSpec, TerminalBackend};

/// Bare fallback backend for when no terminal multiplexer is available.
/// Runs the first pane's command in the current terminal. No splits,
/// no tab management.
#[derive(Default)]
pub struct BareBackend;

impl BareBackend {
    pub fn new() -> Self {
        Self
    }
}

impl TerminalBackend for BareBackend {
    fn supports_run_in_pane(&self) -> bool {
        false
    }

    fn open_workspace(&self, path: &Path, panes: &[PaneSpec], verbose: bool) -> Result<String> {
        if verbose {
            eprintln!(
                "No supported terminal or multiplexer detected. \
                 Running in bare mode (single pane, no splits)."
            );
        }

        // Find the first pane with a command (typically the agent pane)
        let pane = panes.iter().find(|p| p.command.is_some()).or(panes.first());

        if let Some(pane) = pane {
            if let Some(ref cmd) = pane.command
                && !cmd.is_empty()
            {
                if verbose {
                    eprintln!("Running: {cmd}");
                }

                let mut child = Command::new("sh")
                    .arg("-c")
                    .arg(cmd)
                    .current_dir(path)
                    .envs(&pane.env)
                    .spawn()?;
                let _ = child.wait();
            }
        } else {
            eprintln!("Workspace opened at {}", path.display());
        }

        // Return the path as a dummy tab ID
        Ok(path.to_string_lossy().into())
    }

    fn close_tab(&self, _tab_id: &str) -> Result<()> {
        // No tab to close in bare mode
        Ok(())
    }

    fn focus_tab(&self, _tab_id: &str) -> Result<bool> {
        // No tab to focus in bare mode
        Ok(false)
    }

    fn run_in_pane(&self, _tab_id: &str, _pane_index: usize, _command: &str) -> Result<()> {
        bail!(
            "bare mode does not support sending commands to panes. \
             Install Zellij or tmux for multi-pane support, or use a \
             supported terminal (Ghostty, iTerm2, WezTerm, Windows Terminal)."
        )
    }
}
