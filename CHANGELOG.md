# Changelog

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
