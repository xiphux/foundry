# Changelog

## v0.2.0

### PR Workflow

- `foundry pr` — push branch and create a GitHub PR via `gh` CLI
- `foundry finish` is now state-driven: automatically merges the PR on GitHub when one was created via `foundry pr`, otherwise merges locally
- `foundry merge` is an alias for `foundry finish`
- `--local` flag on `foundry finish` forces local merge, ignoring any associated PR (recovery path for closed PRs)
- PR info (number, URL) stored in workspace state for reliable detection
- `foundry pr` links existing PRs created manually on GitHub instead of creating duplicates
- Forge abstraction layer (`Forge` trait) designed for future GitLab support

### New Agent Support

- Gemini CLI (`gemini`) — sandbox mode with `-p` prompt and `--resume`
- Aider (`aider`) — interactive REPL
- GitHub Copilot CLI (`copilot`) — `-p` prompt
- Kiro (`kiro`) — formerly Amazon Q Developer CLI, `--resume` support
- OpenCode (`opencode`) — `--prompt` and `--continue` support

### Permission Model

- Three-tier permission system: worktree-scoped sandbox (default for Claude, Codex, Every Code, Gemini), ask-for-permission (default for Aider, Copilot, Kiro, OpenCode), and unrestricted (opt-in)
- `unrestricted_permissions` config option to bypass all sandboxing and auto-approve all actions
- Claude workspaces now enable OS-level sandbox (Seatbelt/bubblewrap) with auto-allow mode
- Claude launches in `acceptEdits` permission mode by default (file edits auto-approved, bash sandboxed)
- Gemini uses `--sandbox --approval-mode=yolo` for sandboxed auto-approval

### Quality of Life

- Config validation: warn on unknown keys in global and project config files (typo detection)
- Agent executable check: verify the configured agent is installed before creating the workspace
- `pr_remote` config option for controlling which remote PR commands push to (auto-detects single remote, defaults to "origin" for multiple)

## v0.1.0

Initial release.

- Manage AI agent workspaces using git worktrees
- Terminal automation for Ghostty, iTerm2, WezTerm, Windows Terminal, Zellij, tmux
- Multi-agent support (Claude, Codex, Every Code) with per-pane configuration
- Two-level TOML config (global + project) with pane layout merging
- Setup/teardown scripts with template variables and deferred execution
- Dynamic port allocation for parallel dev servers
- Branch archiving with `finish`, restore with `restore`
- Workspace activity history (`foundry history`)
- Auto-fetch and fast-forward before branching
- Conversation resume (`--continue`) for supported agents
- Safety checks: `--force` for discard, uncommitted changes detection
- Shell completions via `foundry completions`
