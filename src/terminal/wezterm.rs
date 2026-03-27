use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use super::{PaneSpec, TerminalBackend};
use crate::config::types::SplitDirection;

pub struct WeztermBackend;

/// Info about a pane from `wezterm cli list --format json`.
#[derive(serde::Deserialize)]
struct PaneInfo {
    pane_id: u64,
    tab_id: u64,
}

impl WeztermBackend {
    /// Detect if we're running inside WezTerm.
    pub fn detect() -> Option<Self> {
        let term = std::env::var("TERM_PROGRAM").ok()?;
        if term.eq_ignore_ascii_case("wezterm") {
            Some(Self)
        } else {
            None
        }
    }

    /// Run a `wezterm cli` subcommand and return stdout.
    fn run_cli(args: &[&str]) -> Result<String> {
        let output = Command::new("wezterm")
            .arg("cli")
            .args(args)
            .output()
            .context("failed to run wezterm cli")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("wezterm cli error: {}", stderr.trim());
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Send text to a pane (includes trailing newline to execute).
    fn send_text(pane_id: u64, text: &str) -> Result<()> {
        let pane_arg = pane_id.to_string();
        let text_with_newline = format!("{text}\n");

        // Pass text as a positional argument with --no-paste to send as keystrokes
        Self::run_cli(&[
            "send-text",
            "--pane-id",
            &pane_arg,
            "--no-paste",
            &text_with_newline,
        ])?;
        Ok(())
    }

    /// List all panes and return their info.
    fn list_panes() -> Result<Vec<PaneInfo>> {
        let output = Self::run_cli(&["list", "--format", "json"])?;
        let panes: Vec<PaneInfo> =
            serde_json::from_str(&output).context("failed to parse wezterm cli list output")?;
        Ok(panes)
    }

    /// Find the tab_id for a given pane_id.
    fn tab_id_for_pane(pane_id: u64) -> Result<u64> {
        let panes = Self::list_panes()?;
        panes
            .iter()
            .find(|p| p.pane_id == pane_id)
            .map(|p| p.tab_id)
            .ok_or_else(|| anyhow::anyhow!("pane {pane_id} not found in wezterm pane list"))
    }

    /// Find all pane IDs belonging to a tab, in order.
    fn panes_for_tab(tab_id: u64) -> Result<Vec<u64>> {
        let panes = Self::list_panes()?;
        let mut tab_panes: Vec<u64> = panes
            .iter()
            .filter(|p| p.tab_id == tab_id)
            .map(|p| p.pane_id)
            .collect();
        tab_panes.sort();
        Ok(tab_panes)
    }
}

impl TerminalBackend for WeztermBackend {
    fn open_workspace(&self, path: &Path, panes: &[PaneSpec], verbose: bool) -> Result<String> {
        let path_str = path.to_str().context("invalid worktree path")?;

        if verbose {
            eprintln!("Opening new WezTerm tab...");
        }

        // Spawn the first pane in a new tab
        let first_pane_id: u64 = Self::run_cli(&["spawn", "--cwd", path_str])?
            .parse()
            .context("failed to parse pane id from wezterm cli spawn")?;

        // Map pane names to their WezTerm pane IDs
        let mut pane_ids = std::collections::HashMap::new();

        if let Some(first) = panes.first() {
            pane_ids.insert(first.name.clone(), first_pane_id);
        }

        // Create splits for remaining panes
        if verbose {
            eprintln!("Setting up workspace layout...");
        }

        for pane in panes.iter().skip(1) {
            let split_from = pane
                .split_from
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("pane '{}' has no split_from", pane.name))?;

            let parent_id = pane_ids
                .get(split_from)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "pane '{}' references unknown split_from '{}'",
                        pane.name,
                        split_from
                    )
                })?
                .to_string();

            let dir_flag = match pane
                .direction
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("pane '{}' has no direction", pane.name))?
            {
                SplitDirection::Right => "--right",
                SplitDirection::Down => "--bottom",
            };

            let new_pane_id: u64 = Self::run_cli(&[
                "split-pane",
                "--pane-id",
                &parent_id,
                dir_flag,
                "--cwd",
                path_str,
            ])?
            .parse()
            .context("failed to parse pane id from wezterm cli split-pane")?;

            pane_ids.insert(pane.name.clone(), new_pane_id);
        }

        // Export env vars and run commands in each pane
        for pane in panes {
            if let Some(&pane_id) = pane_ids.get(&pane.name) {
                // Export environment variables
                for (k, v) in &pane.env {
                    Self::send_text(pane_id, &format!("export {k}='{v}'"))?;
                }

                // Run the pane command
                if let Some(ref cmd) = pane.command
                    && !cmd.is_empty()
                {
                    Self::send_text(pane_id, cmd)?;
                }
            }
        }

        // Focus the first pane
        Self::run_cli(&["activate-pane", "--pane-id", &first_pane_id.to_string()])?;

        // Look up the tab ID for later reference
        let tab_id = Self::tab_id_for_pane(first_pane_id)?;

        Ok(tab_id.to_string())
    }

    fn close_tab(&self, tab_id: &str) -> Result<()> {
        if tab_id.is_empty() {
            return Ok(());
        }

        let tab_id_num: u64 = match tab_id.parse() {
            Ok(id) => id,
            Err(_) => return Ok(()), // Invalid tab ID, nothing to close
        };

        // Find all panes in this tab and kill them in reverse order.
        // The first pane (lowest ID) is typically the one running foundry,
        // so killing it last avoids terminating our own process prematurely.
        let pane_ids = match Self::panes_for_tab(tab_id_num) {
            Ok(ids) => ids,
            Err(e) => {
                eprintln!("Warning: failed to list panes for tab {tab_id}: {e}");
                return Ok(());
            }
        };

        if pane_ids.is_empty() {
            eprintln!("Warning: no panes found for tab {tab_id}");
            return Ok(());
        }

        for pane_id in pane_ids.into_iter().rev() {
            if let Err(e) = Self::run_cli(&["kill-pane", "--pane-id", &pane_id.to_string()]) {
                eprintln!("Warning: failed to kill pane {pane_id}: {e}");
            }
        }

        Ok(())
    }

    fn run_in_pane(&self, tab_id: &str, pane_index: usize, command: &str) -> Result<()> {
        if tab_id.is_empty() {
            return Ok(());
        }

        let tab_id_num: u64 = tab_id.parse().context("invalid WezTerm tab ID")?;

        let pane_ids = Self::panes_for_tab(tab_id_num)?;

        let pane_id = pane_ids.get(pane_index).ok_or_else(|| {
            anyhow::anyhow!(
                "pane index {pane_index} out of range (tab has {} panes)",
                pane_ids.len()
            )
        })?;

        Self::send_text(*pane_id, command)?;
        Ok(())
    }

    fn focus_tab(&self, tab_id: &str) -> Result<bool> {
        if tab_id.is_empty() {
            return Ok(false);
        }

        let tab_id_num: u64 = match tab_id.parse() {
            Ok(id) => id,
            Err(_) => return Ok(false),
        };

        let pane_ids = Self::panes_for_tab(tab_id_num)?;

        if let Some(&first_pane) = pane_ids.first() {
            Self::run_cli(&["activate-pane", "--pane-id", &first_pane.to_string()])?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
