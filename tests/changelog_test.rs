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

fn parse(text: &str) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();
    for line in text.lines() {
        // `## ` but not `### `: a subheading is part of the body.
        let is_heading = line.starts_with("## ") && !line.starts_with("### ");
        if is_heading {
            sections.push(Section {
                heading: line[3..].trim().to_string(),
                body: String::new(),
            });
        } else if let Some(current) = sections.last_mut() {
            current.body.push_str(line);
            current.body.push('\n');
        }
    }
    sections
}

/// `vX.Y.Z` → (X, Y, Z), ignoring any prerelease suffix.
fn version_key(heading: &str) -> Option<(u64, u64, u64)> {
    let rest = heading.strip_prefix('v')?;
    let core = rest.split('-').next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
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
fn parser_drops_the_preamble_above_the_first_heading() {
    let text = "# Changelog\n\nSome prose.\n\n## v0.1.0\n\n- first\n";
    let sections = parse(text);
    assert_eq!(sections.len(), 1);
    assert!(!sections[0].body.contains("Some prose."));
}

#[test]
fn version_key_parses_releases_and_prereleases_only() {
    assert_eq!(version_key("v0.6.1"), Some((0, 6, 1)));
    assert_eq!(version_key("v1.0.0-rc.1"), Some((1, 0, 0)));
    assert_eq!(version_key("Unreleased"), None);
    assert_eq!(version_key("v0.6"), None);
    assert_eq!(version_key("v0.6.1.2"), None);
    assert_eq!(version_key("0.6.1"), None);
}
