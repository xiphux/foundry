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

## Code Quality Rules

**Before committing or after completing a set of changes**, always run:
```bash
cargo fmt && cargo clippy -- -D warnings
```
This is mandatory — CI will reject unformatted code. Since there is no editor format-on-save in the Claude Code workflow, `cargo fmt` must be run explicitly before commits to avoid delayed CI failures.

## Architecture

Foundry is a CLI that manages AI agent workspaces using git worktrees and terminal automation. It shells out to the `git` CLI (not libgit2) for all git operations.

### Module Hierarchy

- **`cli.rs`** — Clap command definitions. **`main.rs`** dispatches commands to workflow modules via `resolve_workspace()` and `load_config()` helpers (avoids boilerplate repetition).
- **`config/`** — Two-level TOML config: global (`~/.foundry/config.toml`) merged with project (`.foundry.toml`). Submodules:
  - `mod.rs` — `ResolvedConfig`, `merge_configs()`, config loading, `expand_tilde`
  - `agents.rs` — `AgentCapabilities` struct, `AGENT_REGISTRY`, `build_agent_command()`. Adding a new agent = one registry entry here.
  - `template.rs` — `TemplateVars`, `validate_template()`, `resolve_template()`. Variables (`{source}`, `{worktree}`, etc.) validated at parse time, resolved at runtime.
  - `validation.rs` — Known config key lists, `warn_unknown_keys()`. Detects typos in TOML config files.
  - `global.rs` / `project.rs` / `types.rs` — Serde structs for config deserialization.
- **`git.rs`** — Thin wrappers around `git` CLI via `run_git()`. All commands use `-C <path>` for explicit repo targeting.
- **`forge/`** — `Forge` trait (analogous to `TerminalBackend`) for PR operations. `GitHubForge` shells out to `gh` CLI. `detect_forge()` resolves the remote and returns the right implementation.
- **`terminal/`** — `TerminalBackend` trait with implementations for Ghostty, iTerm2, WezTerm, tmux, Zellij, Windows Terminal, and a bare fallback. The trait uses `open_workspace()` (not individual split/command calls) because some backends (Ghostty, iTerm2) need all pane references within a single script execution.
- **`workflow/`** — One module per command (start, open, finish, discard, restore, pr, checks, diff, edit, status). Each follows: validate → record state → run scripts → git ops → terminal ops → cleanup state. Shared cleanup logic lives in `cleanup.rs`.
- **`agent_hooks.rs`** — Per-agent workspace setup (Claude settings.local.json, sandbox config, conversation detection). Agent status tracking for the status dashboard.
- **`github.rs`** — GitHub issue fetching (`gh issue view`), issue-to-prompt conversion, slugification for branch names.
- **`history.rs`** — JSONL-based activity log (`~/.foundry/history.jsonl`). Events: started, finished, discarded, restored, pr_created, pr_merged.
- **`registry.rs`** / **`state.rs`** — TOML-backed persistence for project registry (`~/.foundry/projects.toml`) and active workspace state (`~/.foundry/state.toml`).

### Key Design Constraints

- **Ghostty `new tab` bug**: Ghostty 1.x's `new tab` command succeeds but throws a spurious error. The backend works around this by running `new tab` in a separate `osascript` invocation with errors ignored, followed by a 500ms pause, then the layout script.
- **Terminal tab closing must be last**: When finish/discard runs from inside the worktree's tab, closing the tab kills the foundry process. All git cleanup and state persistence must complete before `close_tab()`.
- **State recorded before setup scripts**: `start` writes workspace state before running setup scripts so that `discard` can clean up if setup fails partway through.
- **Config pane merging**: Global config defines pane layout structure. Project config can only override `command` and `env` for existing panes and opt in to `optional` panes. Projects cannot add new panes or change split relationships.
- **Branch archiving**: Branches with commits get archived with a datestamp suffix (`archive/branch-YYYYMMDD`). Branches with no commits are deleted outright to avoid clutter.

### Testing

Most modules have inline `#[cfg(test)]` unit tests. Integration tests in `tests/` create temporary git repos via `tempfile::TempDir`. The `init_test_repo()` helper (in git_test.rs and integration_test.rs) sets up a repo with an initial empty commit on `main`. Terminal and forge operations cannot be tested in CI (require a running terminal / `gh` auth).
