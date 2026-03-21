# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Development Commands

```bash
cargo build                          # Debug build
cargo build --release                # Release build (binary at target/release/foundry)
cargo test                           # Run all tests
cargo test --test git_test           # Run a specific test file
cargo test test_archive_branch       # Run a specific test by name
cargo clippy -- -D warnings          # Lint (CI enforces zero warnings)
cargo fmt                            # Format all code
cargo fmt -- --check                 # Check formatting without modifying
```

CI runs: fmt check, clippy, test, release build — on both Ubuntu and macOS.

## Architecture

Foundry is a CLI that manages AI agent workspaces using git worktrees and terminal automation. It shells out to the `git` CLI (not libgit2) for all git operations.

### Module Hierarchy

- **`cli.rs`** — Clap command definitions. `main.rs` dispatches commands to workflow modules.
- **`config/`** — Two-level TOML config: global (`~/.foundry/config.toml`) merged with project (`.foundry.toml`). `merge_configs()` handles the override logic. Template variables (`{source}`, `{worktree}`, etc.) are validated at parse time but resolved at runtime.
- **`git.rs`** — Thin wrappers around `git` CLI via `run_git()`. All commands use `-C <path>` for explicit repo targeting.
- **`terminal/`** — `TerminalBackend` trait with Ghostty implementation. The trait uses `open_workspace()` (not individual split/command calls) because Ghostty needs all pane references within a single AppleScript execution.
- **`workflow/`** — One module per command (start, open, finish, discard, restore). Each follows: validate → record state → run scripts → git ops → terminal ops → cleanup state.
- **`registry.rs`** / **`state.rs`** — TOML-backed persistence for project registry (`~/.foundry/projects.toml`) and active workspace state (`~/.foundry/state.toml`).

### Key Design Constraints

- **Ghostty `new tab` bug**: Ghostty 1.x's `new tab` command succeeds but throws a spurious error. The backend works around this by running `new tab` in a separate `osascript` invocation with errors ignored, followed by a 500ms pause, then the layout script.
- **Terminal tab closing must be last**: When finish/discard runs from inside the worktree's tab, closing the tab kills the foundry process. All git cleanup and state persistence must complete before `close_tab()`.
- **State recorded before setup scripts**: `start` writes workspace state before running setup scripts so that `discard` can clean up if setup fails partway through.
- **Config pane merging**: Global config defines pane layout structure. Project config can only override `command` and `env` for existing panes and opt in to `optional` panes. Projects cannot add new panes or change split relationships.
- **Branch archiving**: Branches with commits get archived with a datestamp suffix (`archive/branch-YYYYMMDD`). Branches with no commits are deleted outright to avoid clutter.

### Testing

Integration tests in `tests/` create temporary git repos via `tempfile::TempDir`. The `init_test_repo()` helper (in git_test.rs and integration_test.rs) sets up a repo with an initial empty commit on `main`. Unit tests for `derive_worktree_name` are inline in `workflow/restore.rs`. Terminal automation cannot be tested in CI (requires a running Ghostty instance).
