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
use std::sync::OnceLock;

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

/// Which backend `detect_terminal` settled on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BackendKind {
    Ghostty,
    Iterm2,
    Wezterm,
    WindowsTerminal,
    Zellij,
    Tmux,
    Bare,
}

/// Work out which backend to use.
///
/// Ordering is deliberate: native terminals are probed first, and the
/// multiplexers come last as a fallback for launching a *new detached*
/// session — both refuse to run from inside an existing one.
fn detect_kind() -> BackendKind {
    if ghostty::GhosttyBackend::detect().is_some() {
        return BackendKind::Ghostty;
    }
    if iterm2::Iterm2Backend::detect().is_some() {
        return BackendKind::Iterm2;
    }
    if wezterm::WeztermBackend::detect().is_some() {
        return BackendKind::Wezterm;
    }
    if windows_terminal::WindowsTerminalBackend::detect().is_some() {
        return BackendKind::WindowsTerminal;
    }
    // Fallback to terminal multiplexers (Zellij, then tmux)
    if zellij::ZellijBackend::detect().is_some() {
        return BackendKind::Zellij;
    }
    if tmux::TmuxBackend::detect().is_some() {
        return BackendKind::Tmux;
    }
    // Bare fallback — no splits, just run the agent command
    BackendKind::Bare
}

/// Detect the current terminal and return a boxed automation backend.
///
/// The result is memoized for the life of the process. The native backends
/// only read env vars, but the Zellij and tmux probes each spawn a process,
/// and this is called once per workspace by `open --all` and twice by
/// workspace cleanup — so an unmemoized probe re-paid that cost every time,
/// including for users who have neither multiplexer installed.
pub fn detect_terminal() -> Result<Box<dyn TerminalBackend>> {
    static KIND: OnceLock<BackendKind> = OnceLock::new();

    Ok(match KIND.get_or_init(detect_kind) {
        BackendKind::Ghostty => Box::new(ghostty::GhosttyBackend),
        BackendKind::Iterm2 => Box::new(iterm2::Iterm2Backend),
        BackendKind::Wezterm => Box::new(wezterm::WeztermBackend),
        BackendKind::WindowsTerminal => Box::new(windows_terminal::WindowsTerminalBackend),
        BackendKind::Zellij => Box::new(zellij::ZellijBackend),
        BackendKind::Tmux => Box::new(tmux::TmuxBackend),
        BackendKind::Bare => Box::new(bare::BareBackend::new()),
    })
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

    /// How long to pause after opening a workspace before opening the next one
    /// in a batch (`open --all`).
    ///
    /// AppleScript-driven backends drive the terminal through the window
    /// manager and need it to settle before the next tab is created. Backends
    /// that talk to their terminal over a socket or CLI are synchronous and
    /// need no pause at all.
    fn settle_delay(&self) -> std::time::Duration {
        std::time::Duration::ZERO
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
