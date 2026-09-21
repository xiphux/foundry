//! CHANGELOG.md is what the release notes are made of, so a malformed one is a
//! release that says nothing.
//!
//! `dist` builds each GitHub release's body from the section matching the
//! version being tagged. It does not fail when that section is missing — it
//! logs, skips the changelog, and publishes a release carrying only install
//! instructions and a download table. These tests are the guard dist doesn't
//! provide.
//!
//! (The sibling repositories' hundred blank releases came from a different
//! mechanism entirely — GitHub's pull-request-only note generator, on repos
//! that had no CHANGELOG.md at the time. Same symptom, unrelated cause; the
//! two were conflated here.)
//!
//! The version check works because of when foundry bumps: `Cargo.toml` moves to
//! the new version in its own commit (`chore: bump version to 0.6.1`) and the
//! tag is cut on top of it. So "the version in Cargo.toml has a section" is
//! true continuously during development — Cargo.toml still names the last
//! release — and becomes a real assertion on the bump commit, before the tag
//! exists. Deliberately no git plumbing here: CI checks out at depth 1, so a
//! tag-based assertion would silently pass with no tags to check.

use std::path::Path;

/// A `## ` heading and everything under it, up to the next one.
struct Section {
    heading: String,
    body: String,
}

fn changelog() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("CHANGELOG.md");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

fn cargo_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// The code-fence marker a line opens or closes with, if any. CommonMark
/// allows up to three spaces of indent and three or more backticks or tildes.
fn fence_marker(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim_start_matches(' ');
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let character = trimmed.chars().next()?;
    if character != '`' && character != '~' {
        return None;
    }
    let length = trimmed.chars().take_while(|c| *c == character).count();
    if length < 3 {
        return None;
    }
    let info = &trimmed[length..];
    // CommonMark: a BACKTICK fence's info string may not contain a backtick,
    // so ```text with `code` is prose, not a fence opener. Without this,
    // an_unclosed_fence_is_reported fires on a file GitHub renders correctly.
    if character == '`' && info.contains('`') {
        return None;
    }
    Some((&trimmed[..length], info))
}

/// The fence state after `line`, and whether the line was a fence line.
///
/// Both scans over the file go through this, so they cannot disagree about
/// what is inside a fence — which would make the lost-heading report describe
/// a different document from the one `parse` returned.
fn next_fence(fence: Option<String>, line: &str) -> (Option<String>, bool) {
    let Some((marker, info)) = fence_marker(line) else {
        return (fence, false);
    };
    match fence {
        None => (Some(marker.to_string()), true),
        Some(open) => {
            // A closer must use the same character, be at least as long, and
            // carry nothing but whitespace after it: CommonMark allows an info
            // string only on the opener, so ```bash inside an open block is
            // content, not a closer.
            let closes = marker.starts_with(open.chars().next().unwrap_or('`'))
                && marker.len() >= open.len()
                && info.trim().is_empty();
            (if closes { None } else { Some(open) }, true)
        }
    }
}

/// The title of a `## ` heading line, if it is one.
///
/// Matches the JS and Python ports rather than a plain `starts_with("## ")`:
/// any whitespace separates the marker from the title (a tab counts), the
/// title must be non-empty, and `### ` is a subheading. A bare `## ` line was
/// previously read here as a heading with an empty title and there as body
/// text — the same file parsed differently by three implementations of one
/// rule, which is the drift triplication invites.
fn heading_of(line: &str) -> Option<String> {
    // Up to three spaces of indent, matching fence_marker and CommonMark: an
    // indented ATX heading is still a heading, and GitHub renders it as one.
    let trimmed = line.trim_start_matches(' ');
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let rest = trimmed.strip_prefix("##")?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let title = rest.trim();
    if title.is_empty() {
        return None;
    }
    Some(title.to_string())
}

/// Split the file into its `## ` sections, in file order.
///
/// Headings are not recognised inside a fenced code block. An entry showing a
/// TOML or markdown sample can legitimately contain a line starting `## `, and
/// treating it as a section boundary either invents a bogus version or — when
/// the fenced line happens to look like one — silently truncates the real
/// section's body at that point.
fn parse(text: &str) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    // The open fence's marker, or None outside one.
    let mut fence: Option<String> = None;

    for line in text.lines() {
        let (next, is_fence_line) = next_fence(fence, line);
        fence = next;
        if is_fence_line {
            if let Some(current) = sections.last_mut() {
                current.body.push_str(line);
                current.body.push('\n');
            }
            continue;
        }

        let heading = if fence.is_none() {
            heading_of(line)
        } else {
            None
        };
        if let Some(heading) = heading {
            sections.push(Section {
                heading,
                body: String::new(),
            });
        } else if let Some(current) = sections.last_mut() {
            current.body.push_str(line);
            current.body.push('\n');
        }
    }
    sections
}

/// One version component.
///
/// `u64::from_str` accepts a leading `+`, which the JS and Python `\d+` do not,
/// so `v+1.0.0` parsed as a version here and nowhere else. Leading zeros are
/// deliberately still accepted: `\d+` matches `01` in both siblings, so
/// rejecting them here would create the divergence it was meant to remove.
fn parse_part(part: &str) -> Option<u64> {
    if part.starts_with('+') {
        return None;
    }
    part.parse().ok()
}

/// `vX.Y.Z` as a sortable key, or None if the heading is not a version.
///
/// Prereleases are NOT supported, deliberately: this project has never shipped
/// one and does not intend to — anything unreleased is run from git. Accepting
/// the suffix meant ordering it, and semver prerelease precedence is a
/// surprising amount of machinery for a shape nothing produces. `v1.0.0-rc.1`
/// is reported as not a version, which is a clear answer rather than a subtly
/// wrong ordering.
fn version_key(heading: &str) -> Option<(u64, u64, u64)> {
    let rest = heading.strip_prefix('v')?;
    let mut parts = rest.split('.');
    let major = parse_part(parts.next()?)?;
    let minor = parse_part(parts.next()?)?;
    let patch = parse_part(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

#[test]
fn starts_with_the_expected_title() {
    let text = changelog();
    let first = text.lines().next().unwrap_or_default();
    assert_eq!(
        first.trim(),
        "# Changelog",
        "CHANGELOG.md must start with `# Changelog`"
    );
}

#[test]
fn every_heading_is_unreleased_or_a_version() {
    for section in parse(&changelog()) {
        assert!(
            section.heading == "Unreleased" || version_key(&section.heading).is_some(),
            "`## {}` is neither `Unreleased` nor a vX.Y.Z version",
            section.heading
        );
    }
}

#[test]
fn unreleased_is_only_ever_the_top_section() {
    // An Unreleased below a released version would mean the entries under it
    // had already shipped.
    for (index, section) in parse(&changelog()).iter().enumerate() {
        if section.heading == "Unreleased" {
            assert_eq!(index, 0, "`## Unreleased` must be the first section");
        }
    }
}

#[test]
fn versions_run_newest_first_with_no_duplicates() {
    let sections = parse(&changelog());
    let mut previous: Option<(u64, u64, u64)> = None;
    let mut seen: Vec<String> = Vec::new();

    for section in &sections {
        let Some(key) = version_key(&section.heading) else {
            continue;
        };
        assert!(
            !seen.contains(&section.heading),
            "`## {}` appears more than once",
            section.heading
        );
        seen.push(section.heading.clone());

        if let Some(previous) = previous {
            assert!(
                previous > key,
                "`## {}` is not below the version above it (newest first)",
                section.heading
            );
        }
        previous = Some(key);
    }
    assert!(!seen.is_empty(), "CHANGELOG.md lists no released versions");
}

#[test]
fn no_released_version_is_empty() {
    // The whole point: a released version with nothing under it is the blank
    // release this file exists to prevent.
    for section in parse(&changelog()) {
        if section.heading == "Unreleased" {
            // An empty Unreleased just means nothing is pending.
            continue;
        }
        assert!(
            !section.body.trim().is_empty(),
            "`## {}` has no entries",
            section.heading
        );
    }
}

#[test]
fn the_version_being_shipped_has_an_entry() {
    let version = cargo_version();
    let heading = format!("v{version}");
    let sections = parse(&changelog());

    let section = sections
        .iter()
        .find(|s| s.heading == heading)
        .unwrap_or_else(|| {
            panic!(
                "Cargo.toml is at {version} but CHANGELOG.md has no `## {heading}` section. \
             Rename `## Unreleased` to `## {heading}` in the version-bump commit — \
             dist builds the release body from that section and will publish an \
             empty release without it."
            )
        });

    assert!(
        !section.body.trim().is_empty(),
        "`## {heading}` is empty, so the {version} release would say nothing"
    );
}

#[test]
fn parser_finds_sections_and_ignores_subheadings() {
    let text = "# Changelog\n\n## Unreleased\n\n### Added\n\n- pending\n\n## v0.1.0\n\n- first\n";
    let sections = parse(text);
    let headings: Vec<&str> = sections.iter().map(|s| s.heading.as_str()).collect();

    assert_eq!(headings, vec!["Unreleased", "v0.1.0"]);
    assert!(sections[0].body.contains("### Added"));
    assert!(sections[0].body.contains("- pending"));
}

#[test]
fn a_heading_inside_a_fenced_block_is_not_a_section() {
    // The dangerous shape: a fenced line that looks like a version. Before
    // fence tracking this passed every structural check AND truncated
    // v1.0.0's body at the fence.
    let text = concat!(
        "# Changelog\n\n",
        "## v1.0.0\n\n",
        "- shows a sample:\n\n",
        "```toml\n",
        "## v0.95.0\n",
        "```\n\n",
        "- and a trailing entry\n\n",
        "## v0.9.0\n\n",
        "- old\n"
    );
    let sections = parse(text);
    let headings: Vec<&str> = sections.iter().map(|s| s.heading.as_str()).collect();

    assert_eq!(headings, vec!["v1.0.0", "v0.9.0"]);
    assert!(sections[0].body.contains("and a trailing entry"));
}

#[test]
fn a_fence_closes_only_on_a_long_enough_marker_of_the_same_character() {
    let text = "# Changelog\n\n## v1.0.0\n\n````\n```\n## v0.5.0\n````\n\n- after\n";
    let sections = parse(text);
    assert_eq!(sections.len(), 1);
    assert!(sections[0].body.contains("- after"));
}

#[test]
fn a_tilde_fence_is_tracked_too() {
    let text = "# Changelog\n\n## v1.0.0\n\n~~~\n## v0.5.0\n~~~\n\n- after\n";
    assert_eq!(parse(text).len(), 1);
}

#[test]
fn parser_drops_the_preamble_above_the_first_heading() {
    let text = "# Changelog\n\nSome prose.\n\n## v0.1.0\n\n- first\n";
    let sections = parse(text);
    assert_eq!(sections.len(), 1);
    assert!(!sections[0].body.contains("Some prose."));
}

#[test]
fn version_key_parses_plain_releases_only() {
    assert_eq!(version_key("v0.6.1"), Some((0, 6, 1)));
    assert_eq!(version_key("Unreleased"), None);
    assert_eq!(version_key("v0.6"), None);
    assert_eq!(version_key("v0.6.1.2"), None);
    assert_eq!(version_key("0.6.1"), None);
}

#[test]
fn a_prerelease_is_not_a_version_here() {
    // Deliberately unsupported rather than half-supported: accepting the
    // suffix means ordering it, and nothing here has ever produced one.
    assert_eq!(version_key("v1.0.0-rc.1"), None);
    assert_eq!(version_key("v1.0.0-"), None);
}

/// Problems that make `parse` SILENTLY LOSE sections, which the checks above
/// cannot see — they only ever examine the sections that survived.
///
/// A code fence opened and never closed swallows every heading beneath it, and
/// a heading typed `##v0.6.1` never matches and joins the section above. Either
/// way the file reads as one enormous section, every structural check passes,
/// and dist builds the release body from a section holding the whole
/// back-catalogue.
fn lost_heading_problems(text: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let mut fence: Option<String> = None;
    let mut opened_at = 0;

    for (index, line) in text.lines().enumerate() {
        let number = index + 1;
        let was_open = fence.is_some();
        let (next, is_fence_line) = next_fence(fence, line);
        fence = next;
        if is_fence_line {
            if !was_open && fence.is_some() {
                opened_at = number;
            }
            continue;
        }
        let body = line.trim_start_matches(' ');
        let indent = line.len() - body.len();
        // `line.len() > 2` after trimming: a line that is exactly `##` has no
        // title to lose, and the JS/Python `^ {0,3}##[^\s#]` needs a third
        // character to test, so they stay quiet on it. Without this, foundry
        // reported a problem its siblings did not -- drift in the very
        // function that exists to catch drift.
        let loose = indent <= 3
            && body.starts_with("##")
            && !body.starts_with("###")
            && body.len() > 2
            && !body[2..].starts_with(char::is_whitespace);
        if fence.is_none() && loose {
            problems.push(format!(
                "line {number}: \"{}\" needs a space after \"##\" to be read as a heading",
                line.trim()
            ));
        }
    }

    if fence.is_some() {
        problems.push(format!(
            "the code fence opened on line {opened_at} is never closed, so every heading below it was read as body text"
        ));
    }
    problems
}

#[test]
fn the_committed_changelog_loses_no_headings() {
    assert_eq!(lost_heading_problems(&changelog()), Vec::<String>::new());
}

#[test]
fn an_unclosed_fence_is_reported() {
    // Without this the parse yields ONE section holding the whole
    // back-catalogue and every other check in this file still passes.
    let text = concat!(
        "# Changelog\n\n",
        "## v1.1.0\n\n",
        "- shows a sample:\n\n",
        "```toml\n",
        "key = 1\n\n",
        "## v1.0.0\n\n",
        "- old\n"
    );
    assert_eq!(parse(text).len(), 1);
    let problems = lost_heading_problems(text);
    assert_eq!(problems.len(), 1);
    assert!(problems[0].contains("never closed"), "{problems:?}");
}

#[test]
fn a_heading_with_no_space_after_the_hashes_is_reported() {
    let text = "# Changelog\n\n## v1.1.0\n\n- new\n\n##v1.0.0\n\n- old\n";
    assert_eq!(parse(text).len(), 1);
    let problems = lost_heading_problems(text);
    assert_eq!(problems.len(), 1);
    assert!(problems[0].contains("needs a space"), "{problems:?}");
}

#[test]
fn a_balanced_fence_is_not_reported() {
    let text = "# Changelog\n\n## v1.0.0\n\n```\nx\n```\n\n- a\n";
    assert_eq!(lost_heading_problems(text), Vec::<String>::new());
}

#[test]
fn a_bare_hash_marker_is_not_a_heading() {
    // Matches the JS and Python ports, which require a non-empty title.
    let text = "# Changelog\n\n## v1.0.0\n\n## \n\n- a\n";
    let sections = parse(text);
    let headings: Vec<&str> = sections.iter().map(|s| s.heading.as_str()).collect();
    assert_eq!(headings, vec!["v1.0.0"]);
}

#[test]
fn a_tab_separates_the_marker_from_the_title() {
    let text = "# Changelog\n\n##\tv1.0.0\n\n- a\n";
    let sections = parse(text);
    let headings: Vec<&str> = sections.iter().map(|s| s.heading.as_str()).collect();
    assert_eq!(headings, vec!["v1.0.0"]);
}

#[test]
fn a_heading_indented_up_to_three_spaces_is_a_heading() {
    // CommonMark and GitHub both treat this as a heading. Anchored at column 0
    // it rendered as a section everywhere a reader looked while the parser read
    // it as body text.
    let text = "# Changelog\n\n## v1.1.0\n\n- new\n\n  ## v1.0.0\n\n- old\n";
    let sections = parse(text);
    let headings: Vec<&str> = sections.iter().map(|s| s.heading.as_str()).collect();
    assert_eq!(headings, vec!["v1.1.0", "v1.0.0"]);
    assert_eq!(lost_heading_problems(text), Vec::<String>::new());
}

#[test]
fn a_four_space_indented_line_is_not_a_heading() {
    // Four spaces is an indented code block, which is why the limit is three.
    let text = "# Changelog\n\n## v1.0.0\n\n    ## v0.9.0\n\n- a\n";
    let sections = parse(text);
    assert_eq!(sections.len(), 1);
}

#[test]
fn a_backtick_info_string_containing_a_backtick_is_not_a_fence() {
    // CommonMark forbids it, so GitHub renders this as prose. Treating it as a
    // fence reported an unclosed fence on a file that is fine.
    let text = "# Changelog\n\n## v1.1.0\n\n```text with `code` inside\n\n- an entry\n";
    assert_eq!(lost_heading_problems(text), Vec::<String>::new());
}

#[test]
fn a_line_that_is_only_hashes_is_not_reported() {
    // The JS and Python `^ {0,3}##[^\s#]` need a third character to test, so
    // they stay quiet here; this used to be the one place foundry disagreed.
    let text = "# Changelog\n\n## v1.0.0\n\n##\n\n- a\n";
    assert_eq!(lost_heading_problems(text), Vec::<String>::new());
}

#[test]
fn a_leading_plus_is_not_a_version() {
    // `u64::from_str` accepts it; the sibling regexes do not.
    assert_eq!(version_key("v+1.0.0"), None);
    // Leading zeros stay accepted, because `\d+` matches `01` in both siblings.
    assert_eq!(version_key("v01.0.0"), Some((1, 0, 0)));
}

/// The `host:` job block of release.yml, which publishes the GitHub Release.
fn host_job() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows/release.yml");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));

    let mut block = String::new();
    let mut inside = false;
    for line in text.lines() {
        let is_job_header =
            line.starts_with("  ") && !line.starts_with("   ") && line.trim_end().ends_with(':');
        if is_job_header {
            if inside {
                break;
            }
            inside = line.trim() == "host:";
        }
        if inside {
            block.push_str(line);
            block.push('\n');
        }
    }
    assert!(!block.is_empty(), "release.yml has no `host:` job");
    block
}

/// The changelog guard only matters if a failing `cargo test` actually stops a
/// release. It does not by default: `host` is generated with no dependency on
/// `custom-ci-gate`, and its `if:` accepts `skipped` for the build jobs — which
/// is precisely what a FAILED gate produces, since a failed dependency skips its
/// dependents. Any red check would then publish a GitHub Release with no
/// binaries and 404 install links.
///
/// release.yml is dist-generated and marked do-not-edit, but
/// `allow-dirty = ["ci"]` in dist-workspace.toml sanctions hand edits to it —
/// and means `dist generate` will not warn when it reverts one. These two
/// assertions are what notices.
#[test]
fn a_failing_ci_gate_blocks_the_release() {
    let host = host_job();

    // On a LIVE line, not anywhere in the block: `host.contains(...)` alone is
    // satisfied by `#      - custom-ci-gate`, which would keep this green while
    // disconnecting the gate. `dist generate` would delete the line rather than
    // comment it, so the machine regression was covered either way — a hand
    // edit was not.
    let needs_gate = host
        .lines()
        // trim(), not trim_start(): `- custom-ci-gate  ` with trailing spaces
        // is still valid YAML and still live, and would otherwise fail here.
        .any(|line| line.trim() == "- custom-ci-gate");
    assert!(
        needs_gate,
        "release.yml's `host` job must `needs: custom-ci-gate` on a live line, \
         or a failing cargo test cannot stop the release. Block was:\n{host}"
    );

    let requires_success = host.lines().any(|line| {
        line.contains("needs.custom-ci-gate.result == 'success'")
            && !line.trim_start().starts_with('#')
    });
    assert!(
        requires_success,
        "release.yml's `host` job must require custom-ci-gate to have SUCCEEDED \
         (not merely not-failed), since a failed dependency reports as skipped. \
         Block was:\n{host}"
    );
}

#[test]
fn a_closing_fence_may_not_carry_an_info_string() {
    // CommonMark allows an info string only on the opener, so ```bash inside an
    // open block is content. Treating it as a closer made the `## ` line after
    // it a section heading, splitting the body at a line GitHub renders as code.
    let text = concat!(
        "# Changelog\n\n",
        "## v1.0.0\n\n",
        "```\n",
        "code\n",
        "```bash\n",
        "## v0.5.0\n",
        "```\n\n",
        "- a\n"
    );
    let sections = parse(text);
    assert_eq!(sections.len(), 1);
    assert_eq!(sections[0].heading, "v1.0.0");
}
