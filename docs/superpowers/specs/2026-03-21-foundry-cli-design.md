# Foundry CLI — Design Specification

A Rust command-line utility for managing AI agent workspaces using git worktrees and terminal automation.

## Problem

Setting up a parallel AI agent workspace involves multiple manual steps: creating a feature branch, checking out a worktree, copying project-specific files, installing dependencies, and opening a terminal layout with the right tools. Tearing down afterward is similarly tedious. Foundry automates the full lifecycle.

## Commands

| Command | Purpose |
|---|---|
| `foundry start <name>` | Create branch, worktree, run setup scripts, open terminal workspace. Idempotent — if the worktree already exists, skips to opening the workspace. |
| `foundry open [name]` | Reopen terminal workspace for an existing worktree. Lists active worktrees if no name given. |
| `foundry finish [name]` | Merge feature branch to main/master, run teardown scripts, close terminal tab, delete worktree, archive branch. |
| `foundry discard [name]` | Run teardown scripts, close terminal tab, delete worktree, archive branch. No merge. |
| `foundry projects list` | List registered projects and their paths. |
| `foundry projects add <name> <path>` | Manually register a project. |
| `foundry projects remove <name>` | Unregister a project. |
| `foundry list` | List all active foundry-managed workspaces across all projects. |

### Global flags

- `--project <name>` — specify the project explicitly (for commands that operate on a project).
- `--verbose` — show detailed output for each step.
- `--yes` — skip confirmation prompts (e.g., `discard` with uncommitted changes).

### Name resolution

- `start` requires `<name>`.
- `open`, `finish`, and `discard` accept an optional `[name]`. If omitted, the tool infers the worktree from the current working directory (if inside a known foundry-managed worktree). If inference fails, name is required.
- All commands accept `--project <name>` to specify the project explicitly. If omitted, the project is inferred from the current git repo root.

## Architecture

Single Rust crate with modular internals. All git operations shell out to the `git` CLI.

```
foundry/
├── Cargo.toml
├── src/
│   ├── main.rs              # entry point, CLI dispatch
│   ├── cli.rs               # clap definitions
│   ├── config/
│   │   ├── mod.rs            # config loading, merging global + project
│   │   ├── global.rs         # global config types
│   │   └── project.rs        # project config types
│   ├── git.rs                # git CLI wrapper
│   ├── terminal/
│   │   ├── mod.rs            # TerminalAutomation trait, detection
│   │   └── ghostty.rs        # Ghostty AppleScript implementation
│   ├── workflow/
│   │   ├── mod.rs
│   │   ├── start.rs          # start command orchestration
│   │   ├── open.rs           # open command orchestration
│   │   ├── finish.rs         # finish command orchestration
│   │   └── discard.rs        # discard command orchestration
│   ├── registry.rs           # project name → path registry
│   └── errors.rs             # error types
```

### Dependencies

- `clap` (with `derive` feature) — CLI parsing and shell completion generation
- `serde` + `toml` — config deserialization
- `anyhow` — error handling with context
- `which` — locating executables (git, osascript)
- `dirs` — cross-platform home directory resolution

## Configuration

### Global config (`~/.foundry/config.toml`)

```toml
# Optional branch prefix — omit or leave empty for no prefix
branch_prefix = "xiphux"

# Default agent command
agent_command = "claude"

# Prefix for archived branches
archive_prefix = "archive"

# Merge strategy: "ff-only" (default) or "merge"
merge_strategy = "ff-only"

# Base directory for worktrees (default: ~/.foundry/worktrees)
worktree_dir = "~/.foundry/worktrees"

# Pane layout — first pane is the initial tab, subsequent panes split from named panes
[[panes]]
name = "agent"
command = "{agent_command}"
[panes.env]
CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS = "1"

[[panes]]
name = "git"
command = "lazygit"
split_from = "agent"
direction = "right"

[[panes]]
name = "shell"
split_from = "git"
direction = "down"

[[panes]]
name = "server"
split_from = "shell"
direction = "right"
optional = true
```

### Project config (`.foundry.toml` in repo root)

```toml
# Setup scripts — run in order after worktree creation
[[scripts.setup]]
name = "Copy env file"
command = "cp {source}/.env {worktree}/.env"

[[scripts.setup]]
name = "Copy dev database"
command = "cp {source}/dev.db {worktree}/dev.db"

[[scripts.setup]]
name = "Install dependencies"
command = "npm install"
working_dir = "{worktree}"

# Teardown scripts — run in order before worktree deletion
[[scripts.teardown]]
name = "Stop docker containers"
command = "docker compose down"
working_dir = "{worktree}"

# Pane overrides — reference global panes by name
# Including a section for an optional pane opts it in
[panes.server]
command = "npm run serve"
```

### Template variables

Available in `command` and `working_dir` fields in both setup/teardown scripts and pane commands.

| Variable | Description |
|---|---|
| `{source}` | Absolute path to the main repo checkout |
| `{worktree}` | Absolute path to the new worktree |
| `{branch}` | Full branch name (with prefix if configured) |
| `{name}` | Short name (without prefix) |
| `{project}` | Project name |
| `{agent_command}` | The configured agent command |

Simple string replacement. No expression evaluation.

### Config merging

- **Scalar values** (branch_prefix, agent_command, merge_strategy, etc.): project overrides global.
- **Pane layout**: global defines the full layout structure (pane names, split relationships, directions). Project config can only override `command` and `env` for existing global panes by name, and opt in to optional panes by including a `[panes.<name>]` section. Project config cannot add new panes, remove non-optional panes, or change split relationships. Optional panes without a project section are skipped.
- **Scripts**: project-only. No global setup/teardown scripts.

### Template variable handling

Variable **names** (e.g., `{source}`, `{worktree}`) are validated at config parse time — an unknown variable name produces an immediate error. Variable **values** are resolved at runtime when the command is executed, since values like `{worktree}` depend on command arguments.

## Project Registry

Stored at `~/.foundry/projects.toml`:

```toml
[projects.myapp]
path = "/Users/xiphux/code/myapp"

[projects.backend]
path = "/Users/xiphux/code/backend"
```

- **Auto-registration**: the first time any `foundry` command runs inside a git repo, the project is registered automatically. The project name is derived from the repo directory name.
- If a directory name collides with an existing project, warn and prompt to use `foundry projects add` with a custom name.
- `foundry projects add <name> <path>` for manual registration.
- `foundry projects remove <name>` for unregistration (warns if active worktrees exist).

## Workspace State

Foundry tracks active workspaces in `~/.foundry/state.toml`:

```toml
[[workspaces]]
project = "myapp"
name = "my-feature"
branch = "xiphux/my-feature"
worktree_path = "/Users/xiphux/.foundry/worktrees/myapp/my-feature"
source_path = "/Users/xiphux/code/myapp"
created_at = "2026-03-21T10:30:00Z"
terminal_tab_id = ""  # terminal-specific identifier, set when workspace is opened
```

The `terminal_tab_id` field stores a terminal-backend-specific identifier (e.g., a Ghostty window/tab reference) that allows `close_tab` to target the correct tab across separate CLI invocations. It is set by `foundry start` and `foundry open` after opening the workspace, and cleared by `foundry finish` and `foundry discard`. If the identifier is empty or stale (tab was closed manually), `close_tab` is a no-op.

This file is the source of truth for which worktrees are foundry-managed (vs. manually created git worktrees). It enables:

- `foundry list` to show all active workspaces across projects.
- `foundry open` (no args) to list workspaces for the current project.
- Shell completions to enumerate valid workspace names.
- Inferring the workspace from the current working directory.

Entries are added by `foundry start` and removed by `foundry finish` and `foundry discard`. On every invocation that reads state (e.g., `list`, `open`, completions), foundry validates that listed worktrees still exist on disk and prunes stale entries.

## Git Operations

All operations shell out to the `git` CLI via `std::process::Command`. All commands use `-C <repo_path>` for explicit path targeting.

| Function | Git command |
|---|---|
| `detect_main_branch()` | Check `git symbolic-ref refs/remotes/origin/HEAD`, fall back to local branches `main` then `master` |
| `create_branch(name)` | `git branch <name>` |
| `create_worktree(path, branch)` | `git worktree add <path> <branch>` |
| `remove_worktree(path)` | `git worktree remove <path>` (with `--force` for discard) |
| `merge_ff_only(branch)` | `git merge --ff-only <branch>` |
| `merge(branch)` | `git merge <branch>` |
| `archive_branch(branch, prefix)` | `git branch -m <branch> <prefix>/<branch>` |
| `has_uncommitted_changes(path)` | `git -C <path> status --porcelain` |
| `current_branch(path)` | `git -C <path> rev-parse --abbrev-ref HEAD` |
| `list_worktrees()` | `git worktree list --porcelain` |

### Branch naming

- With `branch_prefix = "xiphux"`: `foundry start my-feature` creates branch `xiphux/my-feature`, worktree directory `my-feature`.
- Without prefix: branch and directory are both `my-feature`.
- Archive preserves prefix: `xiphux/my-feature` → `archive/xiphux/my-feature`.

## Terminal Automation

### Trait

```rust
trait TerminalAutomation {
    /// Backend-specific handle to a terminal pane.
    type PaneHandle;

    /// Detect the terminal and return an instance if available.
    /// Returns None if this backend is not available in the current environment.
    fn detect() -> Option<Self> where Self: Sized;

    /// Open a new tab with working directory set to `path`. Returns a handle
    /// to the initial pane in the new tab.
    fn open_tab(&self, path: &Path) -> Result<Self::PaneHandle>;

    /// Split an existing pane in the given direction. The new pane inherits
    /// the working directory. Returns a handle to the newly created pane.
    fn split_pane(&self, target: &Self::PaneHandle, direction: SplitDirection) -> Result<Self::PaneHandle>;

    /// Run a command in a specific pane. If env vars are provided, they are
    /// set before the command (e.g., `export K=V && command`).
    fn run_command(&self, target: &Self::PaneHandle, command: &str, env: &HashMap<String, String>) -> Result<()>;

    /// Close the tab identified by a stored tab ID string (from state.toml).
    /// Returns Ok(()) even if the tab no longer exists (already closed manually).
    fn close_tab(&self, tab_id: &str) -> Result<()>;

    /// Return a string identifier for the tab, suitable for persisting in state.toml
    /// so that a future CLI invocation can target this tab with close_tab.
    fn tab_id(&self, pane: &Self::PaneHandle) -> Result<String>;
}
```

During workspace opening, each pane's `PaneHandle` is stored in a map keyed by pane name, so that subsequent `split_from` references can look up the correct handle. After all panes are opened, `tab_id()` is called to persist the tab identifier in `state.toml`.

### Detection

Runtime detection via environment variables:
- `TERM_PROGRAM=ghostty` → Ghostty backend
- No match → error listing supported terminals

No configuration needed. The tool uses whatever terminal it's running inside.

### Ghostty implementation

Uses AppleScript via `osascript` for terminal automation. Reference:
- Docs: https://ghostty.org/docs/features/applescript
- Scripting definition: https://github.com/ghostty-org/ghostty/blob/main/macos/Ghostty.sdef

### Pane environment variables

Per-pane `[panes.env]` tables are supported. Environment variables are set when running the command in that pane (e.g., via `export KEY=VAL && command`).

### Workspace opening flow

1. Open a new tab, `cd` to the worktree path.
2. Walk the pane list in order. For each pane after the first, split from the named `split_from` pane in the given `direction`.
3. Run each pane's `command` (with env vars if specified). Empty command = shell prompt.

### Expected layout (default config)

```
┌──────────────┬──────────────┐
│              │   lazygit    │
│              ├───────┬──────┤
│    claude    │ shell │ dev  │
│              │       │server│
└──────────────┴───────┴──────┘
```

The `server` pane only appears if the project opts in.

## Command Workflows

### `foundry start <name>`

1. Resolve project (from cwd or `--project` flag).
2. Auto-register project if not in registry.
3. Compute branch name (apply prefix if configured).
4. Check if worktree already exists for this name.
   - **Yes** → skip to step 8 (open workspace).
   - **No** → continue.
5. Create branch from main/master HEAD.
6. Create worktree at `<worktree_dir>/<project>/<name>`.
7. Run setup scripts with template variables resolved.
   - On failure: report error, leave worktree in place, exit. Workspace is still recorded in state so `finish`/`discard` can clean it up.
8. Record workspace in `~/.foundry/state.toml`.
9. Detect terminal, open workspace (same flow as `foundry open`).

### `foundry open [name]`

1. If no name given → list active worktrees for current project, exit.
2. Resolve project.
3. Verify worktree exists → error if not.
4. Detect terminal.
5. Open new tab at worktree path.
6. Walk pane layout, execute splits, run commands.

### `foundry finish [name]`

1. If no name given → infer from cwd (check if inside a known worktree path). Error if inference fails.
2. Resolve project.
3. Verify worktree exists.
4. Check for uncommitted changes in the worktree → error if found.
5. Check for uncommitted changes in the main repo checkout → error if found.
6. Close terminal tab/panes for this workspace via terminal automation (if open). This kills any running processes (dev servers, agents) in those panes.
7. Run teardown scripts via `std::process::Command` (not in terminal panes). These run even if the terminal tab was already closed.
8. From main repo checkout: merge branch using configured strategy.
   - On conflict or ff-only failure → abort merge, report error, exit. Worktree remains intact for the user to fix and retry.
9. Remove worktree (`git worktree remove`).
10. Archive branch (`git branch -m <branch> archive/<branch>`).
11. Remove workspace entry from `~/.foundry/state.toml`.

### `foundry discard [name]`

1. If no name given → infer from cwd. Error if inference fails.
2. Resolve project.
3. Verify worktree exists.
4. Check for uncommitted changes → **warn** (not error) with confirmation prompt: "Worktree has uncommitted changes. Discard anyway? [y/N]" (skipped with `--yes` flag).
5. Close terminal tab/panes for this workspace via terminal automation (if open).
6. Run teardown scripts via `std::process::Command`.
7. Remove worktree (`git worktree remove --force`).
8. Archive branch.
9. Remove workspace entry from `~/.foundry/state.toml`.

## Error Handling

Uses `anyhow` for error propagation with contextual messages.

| Scenario | Behavior |
|---|---|
| Branch already exists but no worktree | Error: "branch `x` already exists" |
| Worktree exists but branch was deleted externally | Error: "worktree exists but branch is missing" |
| `git` not found on PATH | Error at startup |
| `osascript` not found (non-macOS) | Error when terminal automation is needed |
| Unknown template variable in config | Error at config parse time |
| Project name collision during auto-register | Warn, prompt to use `foundry projects add` |
| `foundry finish` while on main/master | Error: "already on main" |
| Worktree directory exists but isn't git-managed | Error: "not a git worktree" |
| Uncommitted changes in worktree (finish) | Error with instruction to commit or stash |
| Uncommitted changes in main repo (finish) | Error with path to main repo |
| Uncommitted changes in worktree (discard) | Warning with confirmation prompt |
| Merge conflicts (finish) | Abort merge, report error, worktree intact |

### Exit codes

- `0` — success
- `1` — general error
- `2` — usage error (bad arguments)

## Shell Completions

Generated via `clap`'s built-in completion support for bash, zsh, and fish. Completions for `open`, `finish`, and `discard` dynamically list active worktrees for the current project.

## Cross-Platform Considerations

- Path handling via `std::path::PathBuf` and `dirs` crate.
- Platform-specific code isolated to the `terminal` module.
- Ghostty AppleScript automation is macOS-only; future terminal backends can support other platforms.

## Deferred (post-v1)

- **PR workflow**: `foundry pr` to push and create a GitHub PR via `gh`. Separate command to merge the PR and clean up the workspace.
- **Resumable setup scripts**: track which setup steps completed, retry from point of failure.
- **Additional terminal backends**: kitty, WezTerm, iTerm2, etc.
- **`--dry-run` flag**: show what a command would do without executing.
- **Config schema validation**: strict vs. lenient parsing of unknown keys for forward compatibility.
- **Interrupted setup recovery**: marker file (e.g., `.foundry-setup-incomplete`) to detect and handle partial setup state.
