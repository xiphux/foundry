# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Development Commands

```bash
cargo build                          # Debug build
cargo build --release                # Release build (binary at target/release/foundry)
cargo test                           # Run all tests
cargo test --test git_test           # Run a specific test file
cargo test test_archive_branch       # Run a specific test by name
cargo clippy --all-targets -- -D warnings  # Lint, tests included (CI enforces zero warnings)
cargo fmt                            # Format all code
cargo fmt -- --check                 # Check formatting without modifying
cargo llvm-cov --summary-only        # Line coverage (CI fails below the floor in ci.yml)
cargo machete                        # Declared crates nothing uses (CI gates on it)
```

CI runs: fmt check, clippy, test, release build (all `--locked`) on Ubuntu x86_64 and arm64, macOS (plus an x86_64 macOS cross-build) and Windows — every platform release ships to — plus a coverage floor, `cargo audit`, `cargo machete`, a gitleaks secret scan and a workflow lint.

The workflow lint is actionlint (workflow mistakes, including shellcheck over `run:` blocks) and zizmor (workflow security). Run them locally the way CI does:

```bash
docker run --rm -v "$PWD:/repo" -w /repo rhysd/actionlint:latest -color
docker run --rm -v "$PWD:/repo" -w /repo ghcr.io/zizmorcore/zizmor:latest .
```

Both are clean, and each exception is recorded with its reasoning in `.github/actionlint.yaml` or `.github/zizmor.yml` — nearly all of them dist-generated `release.yml` content, ignored line by line so the hand-maintained Scoop jobs in that file are still covered. Every action is pinned to a commit SHA with the version in a trailing comment, which is what Dependabot updates; `dtolnay/rust-toolchain` is pinned to its `v1` tag and passed `toolchain: stable` explicitly, because the rolling `stable` branch is what used to supply that default.

`cargo machete` runs beside `cargo audit` in the same job: both read the manifest rather than the build. It parses sources rather than compiling them, so a crate reached only through a macro or a `cfg` this platform skips can read as unused — that goes in `[package.metadata.cargo-machete]` in Cargo.toml with its reason, not behind `--skip-analysis`. Nothing needs it today.

Renovate proposes GitHub Actions and Cargo updates (`.github/renovate.json5`) and merges patches (except 0.0.x) and minors (except 0.x) once CI is green, so CI is the only gate those updates pass. Crate minors and patches share one PR, since each edits `Cargo.lock` and every PR re-runs the four-way OS matrix; a held 0.x minor leaves that group rather than parking the safe updates with it. Majors arrive as their own PRs and are never auto-merged — Renovate raises them alongside a crate's routine updates rather than in place of them — and they are not rebased while they sit, since nothing will merge them until someone decides. `major-upgrade-check.yml` and `dependabot-automerge.yml` did those jobs before and are now deleted, along with `.github/scripts/major-upgrade-issues.sh`: the weekly `major-upgrade` issues existed only because Dependabot could not offer a major without displacing that crate's routine updates. Dependabot **security** updates are untouched: a repository setting, not a config file, and they now arrive as PRs to merge by hand. `.github/workflows/renovate-nudge.yml` wakes Renovate after a push, because a merge it performs enqueues no job of its own and the scheduled run is four-hourly.

## Code Quality Rules

**Before committing or after completing a set of changes**, always run:
```bash
cargo fmt && cargo clippy --all-targets -- -D warnings
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
- **`agent_hooks.rs`** — Per-agent workspace setup (Claude settings.local.json, worktree-scoped permissions, conversation detection). Agent status tracking for the status dashboard.
- **`github.rs`** — GitHub issue fetching (`gh issue view`), issue-to-prompt conversion, slugification for branch names.
- **`history.rs`** — JSONL-based activity log (`~/.foundry/history.jsonl`). Events: started, finished, discarded, restored, pr_created, pr_merged.
- **`registry.rs`** / **`state.rs`** — TOML-backed persistence for project registry (`~/.foundry/projects.toml`) and active workspace state (`~/.foundry/state.toml`). Both save via `fs_util::write_atomic`.
- **`fs_util.rs`** — `write_atomic()`: temp-file-plus-rename write. Used by `registry.rs`/`state.rs` so a concurrent reader (`status --watch` polls `state.toml` every 2s) can never observe a truncated file — both structs deserialize an empty file as "no entries" rather than failing.

### Key Design Constraints

- **Ghostty `new tab` bug**: Ghostty 1.x's `new tab` command succeeds but throws a spurious error. The backend works around this by running `new tab` in a separate `osascript` invocation with errors ignored, followed by a 500ms pause, then the layout script.
- **Terminal tab closing must be last**: When finish/discard runs from inside the worktree's tab, closing the tab kills the foundry process. All git cleanup and state persistence must complete before `close_tab()`. The exception is the worktree-removal recovery path, which closes the tab early to stop whatever is writing into the worktree — it is guarded on the process not running inside that worktree.
- **A partly-failed `git worktree remove` is not a failure**: git drops its registration under `.git/worktrees/` even when deleting the directory fails, so files a dev server recreated mid-delete leave nothing for a retry to remove. `git::worktree_registered` distinguishes that from a removal git refused outright; only the latter is fatal.
- **State recorded before setup scripts**: `start` writes workspace state before running setup scripts so that `discard` can clean up if setup fails partway through.
- **Config pane merging**: Global config defines pane layout structure. Project config can only override `command` and `env` for existing panes and opt in to `optional` panes. Projects cannot add new panes or change split relationships.
- **Branch archiving**: Branches with commits get archived with a datestamp suffix (`archive/branch-YYYYMMDD`). Branches with no commits are deleted outright to avoid clutter.

### Testing

Most modules have inline `#[cfg(test)]` unit tests. Integration tests in `tests/` create temporary git repos via `tempfile::TempDir`. The `init_test_repo()` helper (in git_test.rs and integration_test.rs) sets up a repo with an initial empty commit on `main`. Terminal and forge operations cannot be tested in CI (require a running terminal / `gh` auth).

- **`tests/e2e_test.rs`** runs the built binary against a throwaway repo, with `FOUNDRY_HOME` pointing at a temp dir and `FOUNDRY_TERMINAL=bare` (the pane command runs in the foreground and returns). A change to a command's behavior belongs here; its `Sandbox` helper sets up repo, config and git identity.
- **`tests/persisted_format_test.rs`** loads state, registry, trust, history and config files in `tests/fixtures/persisted`, written by foundry 0.6. Never regenerate them to make a test pass: a fixture that stops loading means existing users' files would too. Add a new fixture when a format gains fields.
- **`cli.rs`'s tests** pin every command's arguments, and completions for every shell. Update them alongside any CLI change.
