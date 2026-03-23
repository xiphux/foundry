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
| `foundry start <name> --prompt "..."` | Start with a prompt passed to the AI agent |
| `foundry start <name> --prompt-file <path>` | Start with a prompt loaded from a file |
| `foundry open [name]` | Reopen workspace (lists active worktrees if no name) |
| `foundry finish [name]` | Merge to main/master, teardown, clean up |
| `foundry discard [name]` | Teardown and clean up without merging |
| `foundry switch [name]` | Switch to a workspace's terminal tab |
| `foundry restore [branch]` | Restore workspace from an archived branch |
| `foundry status` | Show status dashboard of all workspaces |
| `foundry diff [name]` | Show changes in a workspace vs main |
| `foundry diff [name] --stat` | Show file change summary vs main |
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

# Default AI agent: "claude" (default), "codex", "every-code", or "custom"
# Used for the default pane layout when no panes have explicit agent fields
agent = "claude"

# Prefix for archived branches (default: "archive")
archive_prefix = "archive"

# Merge strategy: "ff-only" (default) or "merge"
merge_strategy = "ff-only"

# Base directory for worktrees (default: "~/.foundry/worktrees")
worktree_dir = "~/.foundry/worktrees"
```

### Default Pane Layout

If no `[[panes]]` are configured, foundry uses a simple default layout — the agent on the left and a plain shell on the right:

```
┌──────────────┬──────────────┐
│              │              │
│    agent     │    shell     │
│              │              │
└──────────────┴──────────────┘
```

### Custom Pane Layout

You can define your own layout in the global config. The first pane becomes the initial tab. Subsequent panes split from a named parent pane in the given direction.

```toml
# ~/.foundry/config.toml

[[panes]]
name = "agent"
agent = "claude"
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

#### Multi-Agent Layout

You can run multiple different agents in a single workspace by setting `agent` on different panes. Each agent type can only appear once per workspace.

```toml
[[panes]]
name = "developer"
agent = "claude"

[[panes]]
name = "reviewer"
agent = "codex"
split_from = "developer"
direction = "right"

[[panes]]
name = "shell"
split_from = "developer"
direction = "down"
```

This produces the following layout (with `server` pane only if the project opts in):

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
| `agent` | No | Agent to run in this pane (auto-generates command with permissions and prompt passthrough) |
| `command` | No | Command to run (empty = shell prompt, ignored if `agent` is set) |
| `split_from` | No* | Name of the pane to split from |
| `direction` | No* | Split direction: `"right"` or `"down"` |
| `optional` | No | If `true`, only included when the project opts in (default: `false`) |
| `[panes.env]` | No | Environment variables to set for this pane |

\* Required for all panes except the first (which becomes the tab).

**Important:** Always use `agent` instead of `command` for AI coding agents. Using `agent = "claude"` ensures foundry sets up permissions, status tracking, and prompt passthrough. If you use `command = "claude"` directly, foundry will warn you to switch to the `agent` field.

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
deferred = true  # runs in the shell pane after the workspace opens

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
# Override the default agent for this project
agent = "codex"

# Override the merge strategy for this project
merge_strategy = "merge"
```

### Deferred Setup Scripts

Setup scripts marked with `deferred = true` run in the shell pane **after** the workspace opens, rather than blocking before it opens. This lets you start working while slower scripts (like `npm install`) run in the background.

Deferred scripts are chained together with `&&` and sent to the first pane that has no command configured (typically the "shell" pane). Non-deferred scripts run in order before the workspace opens, as usual.

### Template Variables

The following variables can be used in `command` and `working_dir` fields in scripts and pane commands:

| Variable | Description |
|---|---|
| `{source}` | Absolute path to the main repo checkout |
| `{worktree}` | Absolute path to the worktree |
| `{branch}` | Full branch name (with prefix if configured) |
| `{name}` | Short name (without prefix) |
| `{project}` | Project name |

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

## Agent Support

Foundry supports multiple AI coding agents. The agent is configured via `agent` in your global or project config.

| Feature | Claude | Codex | Every Code | Custom |
|---|---|---|---|---|
| Prompt passthrough | Yes | Yes | Yes | No |
| Worktree permissions | Yes (settings.local.json) | Yes (CLI flags) | Yes (CLI flags) | N/A |
| Status tracking | Yes (hooks) | Not yet | Not yet | N/A |
| Settings merge from source | Yes | N/A | N/A | N/A |

**Claude** gets the richest integration: foundry copies your source repo's `.claude/settings.local.json` into the worktree and merges in status-tracking hooks and worktree-scoped permissions (auto-approve file operations within the worktree, deny `git push` and `checkout main`).

**Codex** uses the `--full-auto` flag for autonomous operation (sandbox scoped to workspace, approvals only on failure).

**Every Code** (`every-code`) uses the `--full-auto` flag for autonomous operation (sandbox scoped to workspace, approvals only on failure).

**Custom** agents use whatever command you specify in `agent_command`. Foundry runs it as-is without additional configuration.

## Terminal Support

Foundry detects the terminal automatically from the `TERM_PROGRAM` environment variable.

| Terminal | Platform | Mechanism |
|---|---|---|
| Ghostty | macOS | AppleScript |
| iTerm2 | macOS | AppleScript |
| WezTerm | macOS, Linux, Windows | `wezterm cli` |

All backends support: opening tabs, creating split panes, running commands in panes, closing tabs, and focusing tabs.

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
