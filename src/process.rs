//! Running external tools.

use anyhow::{Context, Result};
use std::process::Command;

/// A command that ran and exited unsuccessfully.
///
/// Typed so a caller can tell a refusal it expects from any other failure by
/// what the tool said — `gh pr checks` exiting non-zero because a PR has no
/// checks, say — rather than by matching the formatted error.
#[derive(Debug)]
pub struct CommandFailed {
    /// The message prefix the caller gave, e.g. "failed to create PR".
    pub failure: String,
    /// The tool's stderr, trimmed.
    pub stderr: String,
}

impl std::fmt::Display for CommandFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.failure, self.stderr)
    }
}

impl std::error::Error for CommandFailed {}

/// Run `cmd` to completion and return its trimmed stdout, or fail with
/// `"{failure}: {stderr}"` (a [`CommandFailed`]) if it exits unsuccessfully.
///
/// One copy of the spawn, check, read-stderr dance every tool wrapper was
/// writing out for itself.
pub fn run_checked(cmd: &mut Command, failure: &str) -> Result<String> {
    let output = cmd
        .output()
        .with_context(|| format!("failed to run {}", cmd.get_program().to_string_lossy()))?;
    if !output.status.success() {
        return Err(CommandFailed {
            failure: failure.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        }
        .into());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn returns_trimmed_stdout() {
        let out = run_checked(Command::new("sh").args(["-c", "echo '  hi  '"]), "x").unwrap();
        assert_eq!(out, "hi");
    }

    #[test]
    fn a_failure_carries_the_prefix_and_stderr() {
        let err = run_checked(
            Command::new("sh").args(["-c", "echo nope >&2; exit 3"]),
            "failed to frob",
        )
        .unwrap_err();
        assert_eq!(err.to_string(), "failed to frob: nope");
        assert_eq!(err.downcast_ref::<CommandFailed>().unwrap().stderr, "nope");
    }

    #[test]
    fn a_missing_program_names_it() {
        let err = run_checked(&mut Command::new("foundry-no-such-tool"), "x").unwrap_err();
        assert!(err.to_string().contains("foundry-no-such-tool"), "{err}");
    }
}
