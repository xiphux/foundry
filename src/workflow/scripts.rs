//! Running the setup and teardown scripts a project config declares.
//!
//! `start`, `restore` and `cleanup` each ran their own copy of this loop. They
//! agreed on the interesting parts — resolve the template, `sh -c` in the
//! worktree — and disagreed on the rest by accident rather than by decision,
//! which is how `restore` ended up running setup scripts without the port
//! variables `start` injects.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::process::Command;

use crate::config::{self, ScriptConfig, TemplateVars};

/// Which phase these scripts belong to.
///
/// The phase decides what a non-zero exit means, so the two travel together
/// rather than as separate parameters that a caller could pair up wrongly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptKind {
    /// Runs before a workspace is handed over. A failure aborts: the workspace
    /// is not usable if its setup did not finish.
    Setup,
    /// Runs while a workspace is being torn down. A failure warns and the rest
    /// still run — there is nothing left to abort into, and stopping early
    /// would skip the remaining cleanup.
    Teardown,
}

impl ScriptKind {
    fn label(self) -> &'static str {
        match self {
            ScriptKind::Setup => "setup",
            ScriptKind::Teardown => "teardown",
        }
    }

    fn aborts_on_failure(self) -> bool {
        self == ScriptKind::Setup
    }
}

/// Run each script in order, in the worktree, with `env` exported.
///
/// `env` carries the workspace's allocated ports so a setup script can bind the
/// same port the pane commands will use.
pub fn run_scripts<'a>(
    scripts: impl IntoIterator<Item = &'a ScriptConfig>,
    kind: ScriptKind,
    vars: &TemplateVars,
    env: &HashMap<String, u16>,
    verbose: bool,
) -> Result<()> {
    for script in scripts {
        let resolved_command =
            config::resolve_template(&script.command, vars).with_context(|| {
                format!(
                    "failed to resolve template in {} script '{}'",
                    kind.label(),
                    script.name
                )
            })?;

        let working_dir = match script.working_dir {
            Some(ref wd) => config::resolve_template(wd, vars)?,
            None => vars.worktree.clone(),
        };

        if verbose {
            eprintln!("Running {} script: {}...", kind.label(), script.name);
        }

        let mut cmd = Command::new("sh");
        cmd.arg("-c")
            .arg(&resolved_command)
            .current_dir(&working_dir);
        for (name, port) in env {
            cmd.env(name, port.to_string());
        }

        let status = cmd
            .status()
            .with_context(|| format!("failed to run {} script '{}'", kind.label(), script.name))?;

        if status.success() {
            continue;
        }

        let code = status.code().unwrap_or(-1);
        if kind.aborts_on_failure() {
            anyhow::bail!(
                "{} script '{}' failed with exit code {code}. \
                 Worktree left in place at {}. \
                 Fix the issue and re-run, or clean up with `foundry discard {}`.",
                kind.label(),
                script.name,
                vars.worktree,
                vars.name,
            );
        }

        eprintln!(
            "Warning: {} script '{}' failed (exit code {code}), continuing...",
            kind.label(),
            script.name
        );
    }

    Ok(())
}
