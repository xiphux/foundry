# Foundry

A command-line tool for managing AI agent workspaces using git worktrees and terminal automation.

Foundry automates the full lifecycle of parallel development workspaces: creating feature branches, setting up worktrees, opening terminal layouts with your tools, and cleaning up when you're done.

## Installation

```bash
cargo install --path .
```

Or build manually:

```bash
cargo build --release
# Binary is at target/release/foundry
```

## Quick Start

```bash
# From inside a git repo:
foundry start my-feature
```

This will:
1. Create a branch (`my-feature`, or `yourprefix/my-feature` if configured)
2. Create a worktree at `~/.foundry/worktrees/<project>/my-feature`
3. Run any configured setup scripts
4. Open a new terminal tab with your configured pane layout

When you're done:

```bash
# Merge to main and clean up:
foundry finish

# Or discard without merging:
foundry discard
```

## Commands

| Command | Description |
|---|---|
| `foundry start <name>` | Create branch, worktree, run setup, open workspace |
| `foundry open [name]` | Reopen workspace (lists active worktrees if no name) |
| `foundry finish [name]` | Merge to main/master, teardown, clean up |
| `foundry discard [name]` | Teardown and clean up without merging |
| `foundry restore [branch]` | Restore workspace from an archived branch |
| `foundry list` | List all active workspaces across all projects |
| `foundry projects list` | List registered projects |
| `foundry projects add <name> <path>` | Register a project |
| `foundry projects remove <name>` | Unregister a project |
| `foundry completions <shell>` | Generate shell completions (bash, zsh, fish) |

### Global Flags

| Flag | Description |
|---|---|
| `--project <name>` | Specify project explicitly (otherwise inferred from cwd) |
| `--verbose` | Show detailed output for each step |
| `--yes` | Skip confirmation prompts |

### Command Details

**`foundry start <name>`** is idempotent — if the worktree already exists, it skips creation and opens the workspace.

**`foundry finish [name]`** and **`foundry discard [name]`** can infer the workspace name from your current directory if you're inside a worktree. When finishing, the merge uses the configured strategy (fast-forward only by default). Branches with commits are archived (e.g., `archive/my-feature-20260321`); branches with no commits are simply deleted.

**`foundry restore [branch]`** accepts a full branch name (`archive/my-feature-20260321`) or just the branch name without the archive prefix. Run with no arguments to see available archived branches.

## Configuration

Foundry uses two levels of TOML configuration:

- **Global config** at `~/.foundry/config.toml` — defaults for all projects
- **Project config** at `.foundry.toml` in each repo root — project-specific overrides

### Global Config

```toml
# ~/.foundry/config.toml

# Optional prefix for branch names (omit for no prefix)
# "my-feature" becomes "xiphux/my-feature"
branch_prefix = "xiphux"

# Command to launch your AI agent (default: "claude")
agent_command = "claude"

# Prefix for archived branches (default: "archive")
archive_prefix = "archive"

# Merge strategy: "ff-only" (default) or "merge"
merge_strategy = "ff-only"

# Base directory for worktrees (default: "~/.foundry/worktrees")
worktree_dir = "~/.foundry/worktrees"

# Terminal pane layout
# The first pane becomes the initial tab. Subsequent panes split from
# a named parent pane in the given direction.

[[panes]]
name = "agent"
command = "{agent_command}"
# Per-pane environment variables (optional)
[panes.env]
SOME_VAR = "value"

[[panes]]
name = "git"
command = "lazygit"
split_from = "agent"
direction = "right"

[[panes]]
name = "shell"
split_from = "git"
direction = "down"

# Optional panes are only included if the project opts in
[[panes]]
name = "server"
split_from = "shell"
direction = "right"
optional = true
```

This configuration produces the following layout (with `server` pane only if the project opts in):

```
┌──────────────┬──────────────┐
│              │   lazygit    │
│              ├───────┬──────┤
│    agent     │ shell │ dev  │
│              │       │server│
└──────────────┴───────┴──────┘
```

### Pane Configuration

| Field | Required | Description |
|---|---|---|
| `name` | Yes | Unique name for this pane |
| `command` | No | Command to run (empty = shell prompt) |
| `split_from` | No* | Name of the pane to split from |
| `direction` | No* | Split direction: `"right"` or `"down"` |
| `optional` | No | If `true`, only included when the project opts in (default: `false`) |
| `[panes.env]` | No | Environment variables to set for this pane |

\* Required for all panes except the first (which becomes the tab).

### Project Config

```toml
# .foundry.toml (in repo root)

# Setup scripts run after worktree creation (in order)
[[scripts.setup]]
name = "Copy env file"
command = "cp {source}/.env {worktree}/.env"

[[scripts.setup]]
name = "Install dependencies"
command = "npm install"
working_dir = "{worktree}"

# Teardown scripts run before worktree deletion (in order)
[[scripts.teardown]]
name = "Stop containers"
command = "docker compose down"
working_dir = "{worktree}"

# Opt in to optional panes and/or override pane commands
[panes.server]
command = "npm run dev"
```

Project config can also override global scalar values:

```toml
# Override the agent command for this project
agent_command = "codex"

# Override the merge strategy for this project
merge_strategy = "merge"
```

### Template Variables

The following variables can be used in `command` and `working_dir` fields in scripts and pane commands:

| Variable | Description |
|---|---|
| `{source}` | Absolute path to the main repo checkout |
| `{worktree}` | Absolute path to the worktree |
| `{branch}` | Full branch name (with prefix if configured) |
| `{name}` | Short name (without prefix) |
| `{project}` | Project name |
| `{agent_command}` | The configured agent command |

### Config Merging Rules

- **Scalar values** (branch_prefix, agent_command, etc.): project overrides global
- **Pane layout**: global defines the structure; project can override `command` and `env` for existing panes by name, and opt in to optional panes
- **Scripts**: project-only (no global scripts)

## Project Registry

Foundry automatically registers projects the first time you run a command inside a git repo. The project name is derived from the directory name.

Projects are stored in `~/.foundry/projects.toml`. You can manage them manually:

```bash
foundry projects list
foundry projects add myapp /path/to/myapp
foundry projects remove myapp
```

## Terminal Support

Foundry currently supports **Ghostty** on macOS. The terminal is detected automatically from the `TERM_PROGRAM` environment variable.

The Ghostty backend uses AppleScript to:
- Open new tabs with the worktree as the working directory
- Create split panes targeting specific parent panes
- Run commands in specific panes
- Close workspace tabs when finishing or discarding

## Shell Completions

Generate completions for your shell:

```bash
# Zsh
foundry completions zsh > ~/.zfunc/_foundry

# Bash
foundry completions bash > /etc/bash_completion.d/foundry

# Fish
foundry completions fish > ~/.config/fish/completions/foundry.fish
```
