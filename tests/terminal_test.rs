#[test]
fn test_split_direction_deserialization() {
    let toml_str = r#"direction = "right""#;
    #[derive(serde::Deserialize)]
    struct Wrapper {
        direction: foundry::config::SplitDirection,
    }
    let w: Wrapper = toml::from_str(toml_str).unwrap();
    assert_eq!(w.direction, foundry::config::SplitDirection::Right);
}

#[test]
fn test_detect_terminal_always_succeeds() {
    // detect_terminal() always returns a backend (bare fallback if nothing else matches)
    let result = foundry::terminal::detect_terminal();
    assert!(result.is_ok());
}

/// detect_terminal is called repeatedly (once per workspace in `open --all`,
/// twice in cleanup). The Zellij and tmux probes each spawn a process, so the
/// decision must be memoized rather than re-derived on every call.
///
/// Only asserts when detection was actually expensive: if a native backend is
/// detected from an env var, or another test already warmed the cache, there
/// is nothing to measure.
#[test]
fn test_detect_terminal_memoizes_expensive_probes() {
    let start = std::time::Instant::now();
    let _ = foundry::terminal::detect_terminal().unwrap();
    let cold = start.elapsed();

    let start = std::time::Instant::now();
    for _ in 0..50 {
        let _ = foundry::terminal::detect_terminal().unwrap();
    }
    let warm_each = start.elapsed() / 50;

    if cold > std::time::Duration::from_millis(1) {
        assert!(
            warm_each < cold / 10,
            "repeat detect_terminal looks unmemoized: cold={cold:?}, warm each={warm_each:?}"
        );
    }
}

/// The memoized decision must not drift between calls.
#[test]
fn test_detect_terminal_is_stable() {
    let first = foundry::terminal::detect_terminal().unwrap();
    let second = foundry::terminal::detect_terminal().unwrap();
    assert_eq!(first.supports_run_in_pane(), second.supports_run_in_pane());
}
