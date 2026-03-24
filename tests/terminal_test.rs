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
