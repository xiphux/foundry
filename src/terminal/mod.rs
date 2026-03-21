pub mod ghostty;

use anyhow::{bail, Result};
use std::collections::HashMap;
use std::path::Path;

use crate::config::types::SplitDirection;

/// Detect the current terminal and return a boxed automation backend.
pub fn detect_terminal() -> Result<Box<dyn TerminalBackend>> {
    if let Some(term) = ghostty::GhosttyBackend::detect() {
        return Ok(Box::new(term));
    }

    let term_program = std::env::var("TERM_PROGRAM").unwrap_or_else(|_| "unknown".into());
    bail!(
        "unsupported terminal: '{term_program}'. Supported terminals: Ghostty"
    )
}

/// Object-safe trait for terminal automation backends.
pub trait TerminalBackend {
    fn open_tab(&self, path: &Path) -> Result<String>;
    fn split_pane(&self, target: &str, direction: &SplitDirection) -> Result<String>;
    fn run_command(&self, target: &str, command: &str, env: &HashMap<String, String>) -> Result<()>;
    fn close_tab(&self, tab_id: &str) -> Result<()>;
}
