//! `.github/zizmor.yml`'s ignores are LINE-scoped, and release.yml is
//! generated — so any hand edit above an ignored line silently re-points it.
//!
//! That has now happened twice, and both times the only thing that noticed was
//! a red `workflow-lint` job after a push: once when `custom-ci-gate` gained a
//! `permissions:` block (+9 lines), and once when `host` gained its dependency
//! on that gate plus the comment explaining it (+20). The findings themselves
//! were unchanged in kind each time; only their positions moved.
//!
//! Line-scoping is deliberate (see zizmor.yml's own header: a whole-file ignore
//! would hide a future finding in the two hand-maintained jobs). This test is
//! the cost of keeping it: it cannot tell whether an ignore is still JUSTIFIED
//! — that is a judgement recorded in the comments there — but it can tell
//! whether it still points at the kind of line it was written for.

use std::path::Path;

fn read(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// Every `- <file>:<line>` ignore in zizmor.yml, paired with the rule it sits
/// under. Rules are the 2-space-indented `name:` keys under `rules:`.
fn ignores() -> Vec<(String, String, usize)> {
    let config = read(".github/zizmor.yml");
    let mut found = Vec::new();
    let mut rule = String::new();

    for line in config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        // `  template-injection:` — a rule name, not `    ignore:` and not an
        // entry.
        let indent = line.len() - line.trim_start().len();
        if indent == 2 && trimmed.ends_with(':') {
            rule = trimmed.trim_end_matches(':').to_string();
            continue;
        }
        if let Some(entry) = trimmed.strip_prefix("- ")
            && let Some((file, number)) = entry.rsplit_once(':')
            && let Ok(number) = number.parse::<usize>()
        {
            found.push((rule.clone(), file.to_string(), number));
        }
    }
    found
}

#[test]
fn every_ignored_line_exists() {
    for (rule, file, number) in ignores() {
        let workflow = read(&format!(".github/workflows/{file}"));
        let total = workflow.lines().count();
        assert!(
            number >= 1 && number <= total,
            "zizmor.yml ignores {file}:{number} under `{rule}`, but that file \
             has {total} lines"
        );
    }
}

#[test]
fn template_injection_ignores_still_point_at_interpolations() {
    let mut checked = 0;
    for (rule, file, number) in ignores() {
        if rule != "template-injection" {
            continue;
        }
        let workflow = read(&format!(".github/workflows/{file}"));
        let line = workflow.lines().nth(number - 1).unwrap_or_default();
        assert!(
            line.contains("${{"),
            "zizmor.yml ignores {file}:{number} for template-injection, but \
             that line does not interpolate anything:\n  {line}\n\
             The file was almost certainly edited above this point and the \
             ignore now covers the wrong line — find the expression it was \
             written for and update the number (and the note in zizmor.yml).",
        );
        checked += 1;
    }
    assert!(checked > 0, "no template-injection ignores found to check");
}
