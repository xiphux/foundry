# Changelog

## v0.4.0

### New Agent Support

- Crush (`crush`) — by Charmbracelet, the original project OpenCode was forked from. Interactive TUI with `--continue` for session resume and `--yolo` for auto-approve.
- Nanocoder (`nanocoder`) — local-first coding agent with positional prompt passing.

### Quality of Life

- `--agent` flag on `foundry start` overrides the configured agent for a single workspace (e.g., `foundry start my-feature --agent claude` to use Claude instead of your default agent). In multi-agent setups, overrides only the primary agent pane.

### Fixes

- Add `~/.gnupg` and source repo `.git` directory to Claude sandbox `allowWrite` — fixes GPG commit signing and fsmonitor errors from pre-commit hooks (lint-staged etc.) running inside the sandbox

## v0.3.1

- Fix: exclude `git` from Claude sandbox so GPG commit signing and worktree `.git` writes work correctly

## v0.3.0

### Status Monitoring

- `foundry status` now shows rich agent activity: current tool being used, last message when idle, error state (rate limit, auth failure, etc.)
- `foundry status --watch` (`-w`) continuously refreshes the dashboard every 2 seconds for live monitoring
- Status tracking powered by a Node.js hook script that captures Claude Code hook events (PostToolUse, Stop, StopFailure, SessionEnd, etc.)
- Stale detection: if a "working" agent hasn't updated in 5+ minutes, the dashboard shows "idle?" as a hint that it may have been interrupted

### Agent Context Injection

- Foundry now injects workspace context into Claude's session via a SessionStart hook, including:
  - Worktree isolation note (safe to make changes freely, git push blocked)
  - Pane descriptions (what the user started in each pane)
  - Allocated port values (so the agent knows where dev servers are running)
- `context` field in `.foundry.toml` for project-specific context with port variable expansion (e.g., `{VITE_PORT}` → `10042`)

### New Commands

- `foundry checks [name]` — show CI check status for a workspace's PR
- `foundry edit [name]` — open workspace in your configured editor (`editor` config option, falls back to `$VISUAL`/`$EDITOR`)
- `foundry browse [name]` — open workspace directory in the system file explorer

### Quality of Life

- `--plan` flag on `foundry start` starts Claude in plan mode (`--permission-mode plan`), requiring plan approval before any edits — useful for complex issues
- `foundry finish` now checks CI status before merging a PR and prompts for confirmation if checks are failing or pending (bypass with `--yes`)
- `foundry finish` auto-fetches and fast-forwards main before local merge (reuses `auto_fetch` config)

### Code Quality

- Deduplicated `main.rs` with `resolve_workspace()` and `load_config()` helpers (594 → 468 lines)
- Split `config/mod.rs` into focused submodules: `agents.rs`, `template.rs`, `validation.rs` (1,066 → 265 lines)
- Extracted `workflow/cleanup.rs` from `workflow/mod.rs` (352 → 188 lines)

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
