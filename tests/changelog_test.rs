//! CHANGELOG.md is what the release notes are made of, so a malformed one is a
//! release that says nothing.
//!
//! `dist` builds each GitHub release's body from the section matching the
//! version being tagged. It does not fail when that section is missing — it
//! just publishes an empty release, which is how sibling repositories ended up
//! with a hundred blank ones. These tests are the guard dist doesn't provide.
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
fn fence_marker(line: &str) -> Option<&str> {
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
    // CommonMark: a BACKTICK fence's info string may not contain a backtick,
    // so ```text with `code` is prose, not a fence opener. Without this,
    // an_unclosed_fence_is_reported fires on a file GitHub renders correctly.
    if character == '`' && trimmed[length..].contains('`') {
        return None;
    }
    Some(&trimmed[..length])
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
        if let Some(marker) = fence_marker(line) {
            match fence.as_deref() {
                None => fence = Some(marker.to_string()),
                // A closer must use the same character and be at least as long.
                Some(open)
                    if marker.starts_with(open.chars().next().unwrap_or('`'))
                        && marker.len() >= open.len() =>
                {
                    fence = None;
                }
                Some(_) => {}
            }
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
        if let Some(marker) = fence_marker(line) {
            match fence.as_deref() {
                None => {
                    fence = Some(marker.to_string());
                    opened_at = number;
                }
                Some(open)
                    if marker.starts_with(open.chars().next().unwrap_or('`'))
                        && marker.len() >= open.len() =>
                {
                    fence = None;
                }
                Some(_) => {}
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
