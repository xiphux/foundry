//! Interactive confirmation.

use anyhow::Result;
use std::io::Write as _;

/// A `[y/N]` prompt the user declined.
///
/// An error rather than `Ok(())`, so a declined `finish` or `discard` exits
/// non-zero: `foundry finish && deploy` must not deploy after a "no". `main`
/// reports it as a plain "Aborted." rather than as a failure.
#[derive(Debug)]
pub struct Aborted;

impl std::fmt::Display for Aborted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("aborted")
    }
}

impl std::error::Error for Aborted {}

/// Ask a yes/no question on stderr; anything but `y` is no.
///
/// stderr, so the question still reaches the terminal when stdout is piped.
pub fn confirm(question: &str) -> Result<bool> {
    eprint!("{question} [y/N] ");
    std::io::stderr().flush()?;
    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    Ok(input.trim().eq_ignore_ascii_case("y"))
}
