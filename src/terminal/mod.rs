mod applescript;
pub mod bare;
pub mod ghostty;
pub mod iterm2;
pub mod tmux;
pub mod wezterm;
pub mod windows_terminal;
pub mod zellij;

use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

use crate::config::types::SplitDirection;

/// A pane to be opened in the terminal workspace.
#[derive(Debug, Clone)]
pub struct PaneSpec {
    /// Unique name for this pane (used for split_from references).
    pub name: String,
    /// Which pane to split from (None = first pane, becomes the tab).
    pub split_from: Option<String>,
    /// Split direction (required if split_from is Some).
    pub direction: Option<SplitDirection>,
    /// Command to run in this pane (None = just a shell).
    pub command: Option<String>,
    /// Environment variables to set for this pane.
    pub env: HashMap<String, String>,
    /// Shell executable to use (e.g., "bash", "powershell").
    /// Only used by backends that need explicit shell selection (Windows Terminal).
    pub shell: Option<String>,
}

/// Detect the current terminal and return a boxed automation backend.
pub fn detect_terminal() -> Result<Box<dyn TerminalBackend>> {
    if let Some(term) = ghostty::GhosttyBackend::detect() {
        return Ok(Box::new(term));
    }

    if let Some(term) = iterm2::Iterm2Backend::detect() {
        return Ok(Box::new(term));
    }

    if let Some(term) = wezterm::WeztermBackend::detect() {
        return Ok(Box::new(term));
    }

    if let Some(term) = windows_terminal::WindowsTerminalBackend::detect() {
        return Ok(Box::new(term));
    }

    // Fallback to terminal multiplexers (Zellij, then tmux)
    if let Some(term) = zellij::ZellijBackend::detect() {
        return Ok(Box::new(term));
    }

    if let Some(term) = tmux::TmuxBackend::detect() {
        return Ok(Box::new(term));
    }

    // Bare fallback — no splits, just run the agent command
    Ok(Box::new(bare::BareBackend::new()))
}

/// Object-safe trait for terminal automation backends.
///
/// The primary method is `open_workspace` which builds the entire terminal
/// layout in one shot. This is necessary because some terminals (like Ghostty)
/// need all pane references within a single script execution — object handles
/// cannot be passed between separate script invocations.
pub trait TerminalBackend {
    /// Whether this backend supports sending commands to existing panes after
    /// the workspace is opened. Multiplexer backends (tmux, zellij) block
    /// during open_workspace, so run_in_pane is not available.
    fn supports_run_in_pane(&self) -> bool {
        true
    }

    /// Open a complete workspace layout with multiple panes.
    /// Returns an identifier for the tab that can be used with `close_tab`.
    fn open_workspace(&self, path: &Path, panes: &[PaneSpec], verbose: bool) -> Result<String>;

    /// Close a tab identified by the given tab_id (from a previous open_workspace call).
    /// Should be a no-op if the tab no longer exists.
    fn close_tab(&self, tab_id: &str) -> Result<()>;

    /// Focus/switch to the tab identified by the given tab_id.
    /// Returns true if the tab was found and focused, false if not found.
    fn focus_tab(&self, tab_id: &str) -> Result<bool>;

    /// Run a command in a specific pane of an already-open workspace.
    /// The pane is identified by name — the backend finds the terminal
    /// whose working directory matches the tab_id and selects the right pane.
    fn run_in_pane(&self, tab_id: &str, pane_index: usize, command: &str) -> Result<()>;
}
