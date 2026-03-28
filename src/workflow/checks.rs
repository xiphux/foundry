use anyhow::Result;
use std::path::Path;

use crate::config::ResolvedConfig;
use crate::forge;
use crate::forge::CheckConclusion;
use crate::state::WorkspaceState;

pub fn run(
    name: &str,
    project_name: &str,
    source_path: &Path,
    config: &ResolvedConfig,
    state: &WorkspaceState,
    verbose: bool,
) -> Result<()> {
    let worktree_path = config.worktree_dir.join(project_name).join(name);

    let workspace = state
        .find_by_worktree_path(&worktree_path.to_string_lossy())
        .ok_or_else(|| anyhow::anyhow!("workspace '{name}' not found in state"))?;

    let pr_number = workspace.pr_number.ok_or_else(|| {
        anyhow::anyhow!("workspace '{name}' has no associated PR. Run `foundry pr {name}` first.")
    })?;

    let branch = &workspace.branch;

    let (forge_impl, _remote) = forge::detect_forge(source_path, config.pr_remote.as_deref())?;

    if verbose {
        eprintln!("Checking CI status for PR #{pr_number} (branch '{branch}')...");
    }

    let status = forge_impl.pr_checks(source_path, branch)?;

    if status.checks.is_empty() {
        eprintln!("PR #{pr_number}: no checks configured");
        return Ok(());
    }

    print_checks(pr_number, &status);

    Ok(())
}

pub fn print_checks(pr_number: u64, status: &forge::ChecksStatus) {
    eprintln!("PR #{pr_number}:");
    for check in &status.checks {
        let (icon, label) = match check.conclusion {
            CheckConclusion::Pass => ("\x1b[32m✓\x1b[0m", "passed"),
            CheckConclusion::Fail => ("\x1b[31m✗\x1b[0m", "failed"),
            CheckConclusion::Pending => ("\x1b[33m⟳\x1b[0m", "pending"),
            CheckConclusion::Skipped => ("\x1b[90m-\x1b[0m", "skipped"),
        };
        eprintln!("  {icon} {:<40} {label}", check.name);
    }

    let passed = status
        .checks
        .iter()
        .filter(|c| c.conclusion == CheckConclusion::Pass)
        .count();
    let failed = status
        .checks
        .iter()
        .filter(|c| c.conclusion == CheckConclusion::Fail)
        .count();
    let pending = status
        .checks
        .iter()
        .filter(|c| c.conclusion == CheckConclusion::Pending)
        .count();

    if status.all_passed() {
        eprintln!("\x1b[32mAll {passed} checks passed.\x1b[0m");
    } else {
        let mut parts = Vec::new();
        if passed > 0 {
            parts.push(format!("{passed} passed"));
        }
        if failed > 0 {
            parts.push(format!("{failed} failed"));
        }
        if pending > 0 {
            parts.push(format!("{pending} pending"));
        }
        eprintln!("{}", parts.join(", "));
    }
}
