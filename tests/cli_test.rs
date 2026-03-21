use std::process::Command;

#[test]
fn test_cli_no_args_shows_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_foundry"))
        .arg("--help")
        .output()
        .expect("failed to run foundry");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("start"));
    assert!(stdout.contains("open"));
    assert!(stdout.contains("finish"));
    assert!(stdout.contains("discard"));
    assert!(stdout.contains("projects"));
    assert!(stdout.contains("list"));
}

#[test]
fn test_cli_start_requires_name() {
    let output = Command::new(env!("CARGO_BIN_EXE_foundry"))
        .arg("start")
        .output()
        .expect("failed to run foundry");
    assert!(!output.status.success());
}
