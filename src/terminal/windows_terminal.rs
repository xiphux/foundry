use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use super::{PaneSpec, TerminalBackend};
use crate::config::types::SplitDirection;

pub struct WindowsTerminalBackend;

/// Which shell syntax to use when constructing pane commands.
enum ShellKind {
    PowerShell(String),
    Bash(String),
}

impl WindowsTerminalBackend {
    /// Detect if we're running inside Windows Terminal.
    ///
    /// Windows Terminal sets the `WT_SESSION` environment variable (a GUID)
    /// for all processes launched within it.
    pub fn detect() -> Option<Self> {
        std::env::var("WT_SESSION").ok().map(|_| Self)
    }

    /// Parse a tab_id into (window_id, pid_dir_path).
    fn parse_tab_id(tab_id: &str) -> Result<(&str, &str)> {
        tab_id
            .split_once('|')
            .ok_or_else(|| anyhow::anyhow!("invalid Windows Terminal tab ID: {tab_id}"))
    }

    /// Escape a string for use inside a wt.exe inline command.
    ///
    /// wt.exe treats bare `;` as its subcommand separator. To include a literal
    /// semicolon in a command passed to a pane, it must be escaped as `\;`.
    fn escape_wt(s: &str) -> String {
        s.replace(';', "\\;")
    }

    /// Determine the shell kind from the user's config value.
    ///
    /// Automatically resolves common mistakes like specifying `git-bash.exe`
    /// (the MinTTY GUI launcher) to the actual embeddable `bash.exe`.
    fn shell_kind(shell: Option<&str>) -> ShellKind {
        match shell {
            None | Some("powershell") | Some("pwsh") => {
                ShellKind::PowerShell(shell.unwrap_or("powershell").to_string())
            }
            Some(path) => {
                let lower = path.to_lowercase();
                if lower.contains("powershell") || lower.contains("pwsh") {
                    ShellKind::PowerShell(path.to_string())
                } else {
                    // Resolve git-bash.exe → bash.exe. git-bash.exe is a MinTTY
                    // GUI launcher that can't be embedded in a terminal pane.
                    // The actual shell is bash.exe in the same installation's
                    // bin/ directory.
                    let resolved = Self::resolve_bash_path(path);
                    ShellKind::Bash(resolved)
                }
            }
        }
    }

    /// Resolve a bash path, handling the common git-bash.exe mistake.
    ///
    /// Git for Windows has two relevant executables:
    /// - `git-bash.exe` (root) — MinTTY GUI launcher, NOT embeddable
    /// - `bin/bash.exe` — the actual MSYS2 bash shell
    ///
    /// If the user specifies `git-bash.exe` or `git-bash`, we resolve it to
    /// the corresponding `bin/bash.exe` in the same Git installation.
    fn resolve_bash_path(path: &str) -> String {
        let p = std::path::Path::new(path);
        let filename = p
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("")
            .to_lowercase();

        if filename == "git-bash.exe" || filename == "git-bash" {
            // Try to find bin/bash.exe relative to git-bash.exe's directory.
            // git-bash.exe is typically at the Git installation root, and
            // bash.exe is in the bin/ subdirectory.
            if let Some(parent) = p.parent() {
                let resolved = parent.join("bin").join("bash.exe");
                if resolved.exists() {
                    eprintln!(
                        "Note: resolved '{}' to '{}' (git-bash.exe is a GUI \
                         launcher and cannot be embedded in terminal panes)",
                        path,
                        resolved.display()
                    );
                    return resolved.to_string_lossy().into();
                }
            }
            eprintln!(
                "Warning: '{}' is a GUI launcher (MinTTY) and cannot be embedded \
                 in terminal panes. Use the path to bash.exe instead, e.g.: \
                 shell = \"C:/Program Files/Git/bin/bash.exe\"",
                path
            );
        }

        path.to_string()
    }

    /// Build the wt.exe args for a PowerShell pane.
    fn build_powershell_args(
        pane: &PaneSpec,
        pid_dir: &Path,
        index: usize,
        ps_exe: &str,
    ) -> Vec<String> {
        let pid_file = pid_dir.join(format!("pane_{index}.pid"));
        let pid_file_str = pid_file.to_string_lossy();

        let mut ps_parts: Vec<String> = Vec::new();

        // Record this shell's PID so close_tab() can kill it later.
        ps_parts.push(format!(
            "$PID | Set-Content -Path '{}'",
            pid_file_str.replace('\'', "''")
        ));

        // Set environment variables.
        for (k, v) in &pane.env {
            let escaped = v.replace('\'', "''");
            ps_parts.push(format!("$env:{k} = '{escaped}'"));
        }

        // Run the pane command if one was specified.
        if let Some(ref cmd) = pane.command {
            if !cmd.is_empty() {
                ps_parts.push(cmd.clone());
            }
        }

        let ps_command = Self::escape_wt(&ps_parts.join("; "));

        vec![
            ps_exe.into(),
            Self::escape_wt("-NoExit"),
            Self::escape_wt("-Command"),
            ps_command,
        ]
    }

    /// Build the wt.exe args for a Bash pane.
    ///
    /// Uses `bash -c '...'` to inject PID tracking and env vars before the
    /// command. For panes with no command, spawns an interactive sub-shell.
    ///
    /// **PID tracking**: MSYS2/Git Bash has its own PID namespace separate
    /// from Windows. `$$` returns the MSYS2 PID which `taskkill` can't use.
    /// We read `/proc/$$/winpid` to get the real Windows PID instead.
    ///
    /// **No `exec`**: MSYS2's `exec` spawns a new Windows process (since
    /// Windows has no native exec), giving it a different Windows PID than
    /// what we recorded. Instead we let bash persist as a wrapper — `taskkill
    /// /T` (tree kill) will kill bash and all its children.
    fn build_bash_args(
        pane: &PaneSpec,
        pid_dir: &Path,
        index: usize,
        bash_exe: &str,
    ) -> Vec<String> {
        let pid_file = pid_dir.join(format!("pane_{index}.pid"));
        // Use forward slashes — bash on Windows handles them fine and it
        // avoids backslash interpretation issues.
        let pid_file_str = pid_file.to_string_lossy().replace('\\', "/");

        let mut parts: Vec<String> = Vec::new();

        // Record the Windows PID. /proc/$$/winpid is MSYS2/Cygwin-specific
        // and gives the actual Windows PID. Falls back to $$ for non-MSYS2.
        parts.push(format!(
            "(cat /proc/$$/winpid 2>/dev/null || echo $$) > '{pid_file_str}'"
        ));

        // Set environment variables.
        for (k, v) in &pane.env {
            let escaped = v.replace('\'', "'\\''");
            parts.push(format!("export {k}='{escaped}'"));
        }

        if let Some(ref cmd) = pane.command {
            if !cmd.is_empty() {
                parts.push(cmd.clone());
            } else {
                parts.push("bash -i".into());
            }
        } else {
            // No command — spawn an interactive sub-shell. The outer bash
            // wrapper persists (holding the recorded PID) so taskkill /T
            // can kill the whole tree.
            parts.push("bash -i".into());
        }

        let bash_command = Self::escape_wt(&parts.join("; "));

        vec![
            bash_exe.into(),
            Self::escape_wt("--login"),
            Self::escape_wt("-c"),
            bash_command,
        ]
    }

    /// Build the wt.exe args for a pane, dispatching to the appropriate shell.
    fn build_pane_args(pane: &PaneSpec, pid_dir: &Path, index: usize) -> Vec<String> {
        match Self::shell_kind(pane.shell.as_deref()) {
            ShellKind::PowerShell(ref exe) => {
                Self::build_powershell_args(pane, pid_dir, index, exe)
            }
            ShellKind::Bash(ref exe) => Self::build_bash_args(pane, pid_dir, index, exe),
        }
    }

    /// Create the temp directory used for PID files and command files.
    fn workspace_dir(path: &Path) -> Result<std::path::PathBuf> {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        path.hash(&mut hasher);
        let dir_name = format!("foundry-{:x}", hasher.finish());
        let dir = std::env::temp_dir().join(dir_name);
        std::fs::create_dir_all(&dir)
            .context("failed to create temp directory for workspace state")?;
        Ok(dir)
    }

    /// Convert a SplitDirection to the corresponding `move-focus` direction string.
    fn direction_str(dir: &SplitDirection) -> &'static str {
        match dir {
            SplitDirection::Right => "right",
            SplitDirection::Down => "down",
        }
    }

    /// Append `move-focus` subcommands to navigate from the first pane to a
    /// target pane via its navigation path.
    fn append_navigation(args: &mut Vec<String>, nav_path: &[String]) {
        // Always reset to the first pane so navigation is absolute.
        args.extend([
            ";".into(),
            "move-focus".into(),
            "--direction".into(),
            "first".into(),
        ]);
        for step in nav_path {
            args.extend([
                ";".into(),
                "move-focus".into(),
                "--direction".into(),
                step.clone(),
            ]);
        }
    }
}

impl TerminalBackend for WindowsTerminalBackend {
    fn supports_run_in_pane(&self) -> bool {
        false
    }

    fn open_workspace(&self, path: &Path, panes: &[PaneSpec], verbose: bool) -> Result<String> {
        if panes.is_empty() {
            bail!("no panes specified");
        }

        let path_str = path.to_str().context("worktree path is not valid UTF-8")?;
        let pid_dir = Self::workspace_dir(path)?;
        let pid_dir_str = pid_dir
            .to_str()
            .context("temp directory path is not valid UTF-8")?
            .to_string();

        if verbose {
            eprintln!("Opening new Windows Terminal tab...");
        }

        // Use `-w 0` to open a tab in the *current* window rather than
        // spawning a brand-new window.
        let mut args: Vec<String> = vec!["-w".into(), "0".into()];

        // Navigation paths: for each pane, the sequence of directional moves
        // needed to reach it from the first pane. Used to position focus
        // before each split-pane subcommand.
        //
        // In chained wt.exe commands, focus does NOT move to the new pane
        // after split-pane — it stays on the parent that was split. So we
        // must explicitly navigate before every split.
        let mut nav_paths: HashMap<String, Vec<String>> = HashMap::new();

        for (i, pane) in panes.iter().enumerate() {
            if i == 0 {
                nav_paths.insert(pane.name.clone(), vec![]);
                args.push("new-tab".into());
            } else {
                let split_from = pane
                    .split_from
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("pane '{}' has no split_from", pane.name))?;

                let dir = pane
                    .direction
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("pane '{}' has no direction", pane.name))?;

                let parent_path = nav_paths.get(split_from).ok_or_else(|| {
                    anyhow::anyhow!(
                        "pane '{}' references unknown split_from '{}'",
                        pane.name,
                        split_from
                    )
                })?;

                // Navigate to the parent pane before splitting.
                Self::append_navigation(&mut args, parent_path);

                args.push(";".into());
                args.push("split-pane".into());
                match dir {
                    SplitDirection::Right => args.push("-V".into()),
                    SplitDirection::Down => args.push("-H".into()),
                }

                // Record the new pane's navigation path:
                // parent's path + one step in the split direction.
                let mut new_path = parent_path.clone();
                new_path.push(Self::direction_str(dir).to_string());
                nav_paths.insert(pane.name.clone(), new_path);
            }

            // Common arguments for both new-tab and split-pane.
            args.extend([
                "-d".into(),
                path_str.into(),
                "--title".into(),
                pane.name.clone(),
            ]);

            // Append the shell-specific command (powershell or bash).
            args.extend(Self::build_pane_args(pane, &pid_dir, i));
        }

        // Focus the first pane (the agent pane) after layout is complete.
        args.extend([
            ";".into(),
            "move-focus".into(),
            "--direction".into(),
            "first".into(),
        ]);

        if verbose {
            eprintln!(
                "Setting up workspace layout with {} pane(s)...",
                panes.len()
            );
        }

        let status = Command::new("wt")
            .args(&args)
            .status()
            .context("failed to run wt.exe — is Windows Terminal installed?")?;

        if !status.success() {
            bail!("wt.exe exited with non-zero status");
        }

        Ok(format!("0|{pid_dir_str}"))
    }

    fn close_tab(&self, tab_id: &str) -> Result<()> {
        if tab_id.is_empty() {
            return Ok(());
        }

        let (_, pid_dir_str) = match Self::parse_tab_id(tab_id) {
            Ok(parts) => parts,
            Err(_) => return Ok(()),
        };

        let pid_dir = Path::new(pid_dir_str);
        if !pid_dir.exists() {
            return Ok(());
        }

        // Collect (index, pid) pairs from the PID files written by each pane.
        let mut pids: Vec<(usize, u32)> = Vec::new();

        if let Ok(entries) = std::fs::read_dir(pid_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if let Some(idx_str) = name_str
                    .strip_prefix("pane_")
                    .and_then(|s| s.strip_suffix(".pid"))
                {
                    if let Ok(idx) = idx_str.parse::<usize>() {
                        if let Ok(content) = std::fs::read_to_string(entry.path()) {
                            if let Ok(pid) = content.trim().parse::<u32>() {
                                pids.push((idx, pid));
                            }
                        }
                    }
                }
            }
        }

        // Kill in reverse order (highest index first) so the pane running
        // foundry (typically index 0) is terminated last.
        pids.sort_by(|a, b| b.0.cmp(&a.0));

        for (_, pid) in &pids {
            // /T kills the entire process tree, /F forces termination.
            if let Err(e) = Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .output()
                .map_err(anyhow::Error::from)
                .and_then(|o| {
                    if o.status.success() {
                        Ok(())
                    } else {
                        Err(anyhow::anyhow!(
                            "{}",
                            String::from_utf8_lossy(&o.stderr).trim()
                        ))
                    }
                })
            {
                eprintln!("Warning: failed to kill pane process {pid}: {e}");
            }
        }

        // Clean up temp directory (PID files, command files).
        let _ = std::fs::remove_dir_all(pid_dir);

        Ok(())
    }

    fn focus_tab(&self, tab_id: &str) -> Result<bool> {
        if tab_id.is_empty() {
            return Ok(false);
        }

        // We use `-w 0` (current window) so there's no named window to target.
        // The tab is already in the user's current window — there's no reliable
        // way to focus a specific tab by index since tabs can be reordered.
        Ok(false)
    }

    fn run_in_pane(&self, _tab_id: &str, _pane_index: usize, _command: &str) -> Result<()> {
        bail!(
            "Windows Terminal does not support sending commands to existing panes. \
             Set 'deferred = false' on panes in your foundry config to run commands \
             at creation time instead."
        )
    }
}
