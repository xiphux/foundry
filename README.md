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

# Or push and create a GitHub PR first:
foundry pr

# Then finish when CI passes (automatically merges the PR):
foundry finish

# Or discard without merging:
foundry discard
```

## Commands

| Command | Description |
|---|---|
| `foundry start <name>` | Create branch, worktree, run setup, open workspace |
| `foundry start --issue <number>` | Start from a GitHub issue (auto-generates name and prompt) |
| `foundry start <name> --prompt "..."` | Start with a prompt passed to the AI agent |
| `foundry start <name> --fetch` | Fetch and fast-forward main before branching |
| `foundry open [name]` | Reopen workspace (resumes agent conversation if available) |
| `foundry open --all` | Reopen all active workspaces for the project |
| `foundry pr [name]` | Push branch and create a GitHub PR |
| `foundry pr [name] --title "..."` | Create PR with a custom title |
| `foundry finish [name]` | Finish workspace: merge PR (if created) or merge locally |
| `foundry finish [name] --local` | Force local merge, ignoring any associated PR |
| `foundry discard [name]` | Teardown and clean up without merging |
| `foundry discard [name] --force` | Discard even if the branch has unmerged commits |
| `foundry switch [name]` | Switch to a workspace's terminal tab |
| `foundry restore [branch]` | Restore workspace from an archived branch |
| `foundry status` | Show status dashboard of all workspaces |
| `foundry diff [name]` | Show changes in a workspace vs main |
| `foundry diff [name] --stat` | Show file change summary vs main |
| `foundry history` | Show workspace activity history |
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
| `--version` | Show version |

### Command Details

**`foundry start <name>`** is idempotent — if the worktree already exists, it skips creation and opens the workspace.

**`foundry pr [name]`** pushes the feature branch to the remote and creates a GitHub PR via the `gh` CLI. If a PR already exists for the branch (created manually on GitHub), foundry links it instead of creating a duplicate. The workspace stays open so you can fix issues if CI fails. The PR title is auto-generated from the branch name unless `--title` is provided.

**`foundry finish [name]`** checks whether a PR was created (via `foundry pr`). If so, it merges the PR on GitHub, fetches to sync local refs, and cleans up. If not, it merges locally using the configured strategy (fast-forward only by default). Branches with commits are archived (e.g., `archive/my-feature-20260321`); branches with no commits are simply deleted. `foundry merge` is an alias for `foundry finish`.

If the associated PR was closed or merged outside of foundry, `finish` will report an error with instructions to either reopen the PR or run `foundry finish --local` to merge locally instead.

**`foundry finish [name]`** and **`foundry discard [name]`** can infer the workspace name from your current directory if you're inside a worktree.

**`foundry restore [branch]`** accepts a full branch name (`archive/my-feature-20260321`) or just the branch name without the archive prefix. Run with no arguments to see available archived branches.

**`foundry open`** resumes the agent's previous conversation (e.g., `claude --continue`) when reopening a workspace that has conversation history. If a new `--prompt` is provided, it starts a fresh conversation instead. `--all` reopens all active workspaces for the project, skipping any that are already open.

**`foundry discard`** requires `--force` (or `-f`) if the branch has unmerged commits, similar to `git branch -D`. Workspaces with no commits can be discarded freely.

**`foundry history`** shows recent workspace lifecycle events (started, finished, discarded, restored) with timestamps and metadata. Use `--limit` to control how many events are shown (default: 20).

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

# Default AI agent: "claude" (default), "codex", "every-code", "gemini",
# "aider", "copilot", "kiro", "opencode", or "custom"
agent = "claude"

# Starting port for dynamic port allocation (default: 10000)
# port_range_start = 20000

# Automatically fetch and fast-forward main before branching (default: false)
# auto_fetch = true

# Remote to fetch from (default: "origin")
# fetch_remote = "upstream"

# Remote to push to for PR commands (default: auto-detect)
# If there's one remote, uses it. If multiple, defaults to "origin".
# pr_remote = "origin"

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

# Allocate unique ports per workspace (available as env vars in all panes)
ports = ["VITE_PORT", "API_PORT"]

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

### Dynamic Port Allocation

When running multiple workspaces in parallel, dev servers compete for the same ports. Foundry can allocate unique ports per workspace and expose them as environment variables.

Add a `ports` array to your project config listing the environment variable names:

```toml
# .foundry.toml
ports = ["VITE_PORT", "API_PORT", "DYNAMODB_PORT"]
```

Each workspace gets a contiguous block of ports (starting from 10000 by default). The variables `$VITE_PORT`, `$API_PORT`, `$DYNAMODB_PORT` are available in all panes and setup scripts. Ports are assigned once at `foundry start` and remain stable for the life of the workspace.

Configure your dev servers to use these variables instead of hardcoded ports (e.g., `vite --port $VITE_PORT`).

To customize the starting port, set `port_range_start` in your global config:

```toml
# ~/.foundry/config.toml
port_range_start = 20000
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

| Agent | Config value | Prompt | Resume | Auto-approve |
|---|---|---|---|---|
| Claude | `claude` | Positional | `--continue` | Worktree permissions |
| Codex | `codex` | Positional | `--resume` | `--full-auto` |
| Every Code | `every-code` | Positional | `--resume` | `--full-auto` |
| Gemini CLI | `gemini` | `-p` flag | `--resume` | `-y` (YOLO) |
| Aider | `aider` | Interactive | No | `--yes` |
| GitHub Copilot | `copilot` | `-p` flag | No | `--yolo` |
| Kiro | `kiro` | Positional | `--resume` | `--trust-all-tools` |
| OpenCode | `opencode` | `--prompt` flag | `--continue` | Via config file |
| Custom | `custom` | N/A | N/A | N/A |

**Claude** gets the richest integration: foundry copies your source repo's `.claude/settings.local.json` into the worktree and merges in status-tracking hooks and worktree-scoped permissions (auto-approve file operations within the worktree, deny `git push` and `checkout main`).

**Codex** and **Every Code** use `--full-auto` for autonomous operation (sandbox scoped to workspace, approvals only on failure).

**Gemini CLI** launches with `-y` (YOLO mode) for automatic action approval. Prompts are passed via `-p` and sessions can be resumed with `--resume`. Note: Gemini may prompt to trust the worktree directory on first use. To avoid this for every workspace, trust the parent worktree directory (e.g., `~/.foundry/worktrees/`) which grants trust to all subdirectories.

**Aider** launches as an interactive REPL with `--yes` for auto-approval. Since Aider auto-exits after processing `--message`, foundry launches it interactively and lets you type prompts directly.

**GitHub Copilot** launches with `--yolo` to enable all permissions. Prompts are passed via `-p`. Note: Copilot may prompt to trust the worktree directory on first use. You can choose "Remember this folder" when prompted, or use `/add-dir` within a session to trust directories permanently.

**Kiro** (formerly Amazon Q Developer CLI) launches with `kiro-cli chat --trust-all-tools` for autonomous tool usage. Prompts are passed as positional arguments and sessions can be resumed with `--resume`.

**OpenCode** launches as an interactive TUI. Prompts are passed via `--prompt` and sessions can be resumed with `--continue`. Auto-approve permissions are configured via `opencode.json` (`"permission": "allow"`) rather than CLI flags.

**Custom** agents use whatever command you specify in `agent_command`. Foundry runs it as-is without additional configuration.

## Terminal Support

Foundry detects the terminal automatically from the `TERM_PROGRAM` environment variable.

| Terminal | Platform | Mechanism |
|---|---|---|
| Ghostty | macOS | AppleScript |
| iTerm2 | macOS | AppleScript |
| WezTerm | macOS, Linux, Windows | `wezterm cli` |
| Windows Terminal | Windows | `wt.exe` |
| Zellij (fallback) | macOS, Linux | `zellij` CLI |
| tmux (fallback) | macOS, Linux | `tmux` CLI |
| Bare (fallback) | any | single pane, no splits |

Native terminal backends open a new tab with splits. If no native backend is detected, foundry falls back to **Zellij** or **tmux** (whichever is available), which take over the current terminal with a multiplexer session. If neither is available, **bare mode** runs the first agent command in the current terminal with no splits.

Windows Terminal does not support `run_in_pane` (deferred pane commands) or `focus_tab` due to `wt.exe` limitations.

### Shell Configuration

On Windows Terminal, you can specify which shell to use in panes:

```toml
# ~/.foundry/config.toml
shell = "C:/Program Files/Git/bin/bash.exe"
```

Supported values: `"powershell"`, `"pwsh"`, or a path to `bash.exe`. If you specify `git-bash.exe`, foundry automatically resolves it to the embeddable `bin/bash.exe` in the same Git installation. Other terminal backends use their default shell and ignore this setting.

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
