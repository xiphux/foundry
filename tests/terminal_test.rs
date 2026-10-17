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
/// decision must be memoized rather than re-derived on every call — which
/// shows as every call handing back the same backend.
#[test]
fn test_detect_terminal_memoizes_its_decision() {
    let first = foundry::terminal::detect_terminal().unwrap();
    for _ in 0..10 {
        let again = foundry::terminal::detect_terminal().unwrap();
        assert!(
            std::ptr::addr_eq(first, again),
            "detect_terminal built a new backend instead of reusing the first"
        );
    }
}

/// `open --all` pauses between workspaces only for backends whose launch is
/// not synchronous. Ghostty and iTerm2 drive the window manager via
/// AppleScript; `wt.exe -w 0` dispatches to an existing window and returns
/// before the tab exists. tmux, Zellij, WezTerm and the bare fallback block
/// until the workspace is up, so a pause there was pure dead time.
#[test]
fn test_settle_delay_only_where_launch_is_async() {
    use foundry::terminal::TerminalBackend as _;

    assert!(
        !foundry::terminal::ghostty::GhosttyBackend
            .settle_delay()
            .is_zero()
    );
    assert!(
        !foundry::terminal::iterm2::Iterm2Backend
            .settle_delay()
            .is_zero()
    );
    assert!(
        !foundry::terminal::windows_terminal::WindowsTerminalBackend
            .settle_delay()
            .is_zero()
    );

    assert!(
        foundry::terminal::tmux::TmuxBackend
            .settle_delay()
            .is_zero()
    );
    assert!(
        foundry::terminal::zellij::ZellijBackend
            .settle_delay()
            .is_zero()
    );
    assert!(
        foundry::terminal::wezterm::WeztermBackend
            .settle_delay()
            .is_zero()
    );
    assert!(
        foundry::terminal::bare::BareBackend
            .settle_delay()
            .is_zero()
    );
}

/// `start` suppresses a deferred pane's command at open time on the promise
/// that it will be sent afterwards, so this answer decides whether that command
/// ever runs. It used to have a `true` default, which meant a backend got the
/// damaging answer by saying nothing; it is now a required method, and this
/// pins what each backend actually answers.
#[test]
fn test_run_in_pane_support_matches_whether_open_blocks() {
    use foundry::terminal::TerminalBackend as _;

    // Return once the layout is built and stay addressable afterwards.
    assert!(foundry::terminal::ghostty::GhosttyBackend.supports_run_in_pane());
    assert!(foundry::terminal::iterm2::Iterm2Backend.supports_run_in_pane());
    assert!(foundry::terminal::wezterm::WeztermBackend.supports_run_in_pane());

    // Block for the life of the session, or have no pane model at all, so
    // there is no "afterwards" to send anything into.
    assert!(!foundry::terminal::tmux::TmuxBackend.supports_run_in_pane());
    assert!(!foundry::terminal::zellij::ZellijBackend.supports_run_in_pane());
    assert!(!foundry::terminal::bare::BareBackend.supports_run_in_pane());
    // Hands the command to an existing wt.exe and returns before the tab exists.
    assert!(!foundry::terminal::windows_terminal::WindowsTerminalBackend.supports_run_in_pane());
}
