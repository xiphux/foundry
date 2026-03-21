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
- **Pane layout**: global defines the layout structure. Project overrides individual panes by name (command, env vars). Project can opt in to optional panes by including a `[panes.<name>]` section. Optional panes without a project section are skipped.
- **Scripts**: project-only. No global setup/teardown scripts.

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

## Git Operations

All operations shell out to the `git` CLI via `std::process::Command`. All commands use `-C <repo_path>` for explicit path targeting.

| Function | Git command |
|---|---|
| `detect_main_branch()` | Check local branches for `main`, then `master` |
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
    fn detect() -> bool;
    fn open_tab(&self, path: &Path) -> Result<()>;
    fn split_pane(&self, direction: SplitDirection) -> Result<()>;
    fn run_command(&self, command: &str) -> Result<()>;
    fn close_tab(&self) -> Result<()>;
}
```

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
   - On failure: report error, leave worktree in place, exit.
8. Detect terminal, open workspace (same flow as `foundry open`).

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
6. Close Ghostty tab/panes for this workspace (if open).
7. Run teardown scripts.
8. From main repo checkout: merge branch using configured strategy.
   - On conflict or ff-only failure → abort merge, report error, exit. Worktree remains intact for the user to fix and retry.
9. Remove worktree (`git worktree remove`).
10. Archive branch (`git branch -m <branch> archive/<branch>`).

### `foundry discard [name]`

1. If no name given → infer from cwd. Error if inference fails.
2. Resolve project.
3. Verify worktree exists.
4. Check for uncommitted changes → **warn** (not error) with confirmation prompt: "Worktree has uncommitted changes. Discard anyway? [y/N]"
5. Close Ghostty tab/panes (if open).
6. Run teardown scripts.
7. Remove worktree (`git worktree remove --force`).
8. Archive branch.

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
