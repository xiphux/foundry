use anyhow::Result;

use crate::forge;
use crate::forge::CheckConclusion;

pub fn run(ctx: &mut super::WorkflowCtx, name: &str) -> Result<()> {
    // Resolved from state alone: this command reads the PR number and branch
    // and then talks to the forge, touching no file in the worktree. Requiring
    // the directory would refuse to report CI status for a live PR just because
    // the worktree had been removed out of band.
    let workspace = ctx.recorded_workspace(name)?;
    let (source_path, config, verbose) = (ctx.source_path, ctx.config, ctx.verbose);

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
