# Changelog

## Unreleased

### Changed

- `foundry finish` now actually archives a locally merged branch (the `--local` / no-PR path; a PR-merged branch is still deleted, since the forge already removed the remote one). It always reported `Branch 'x' archived.` but deleted the branch instead: the archive-or-delete decision was taken *after* the merge, when `main..branch` is necessarily empty, so every finished branch looked commitless. `foundry restore` consequently had nothing to restore from. The decision is now made before the merge, and the message reports the real archived name (`Branch 'x' archived as archive/x-20260810.`).
- Claude now launches with `--permission-mode auto` instead of `acceptEdits` plus the OS sandbox. Auto mode uses model analysis to approve most permission requests, which covers the cases the sandbox was working around (GPG signing, worktree `.git` writes, pre-commit hooks) without the sandbox's escape hatches. Foundry no longer writes a `sandbox` block into the worktree's `.claude/settings.local.json`; worktree-scoped allow/deny rules and status hooks are unchanged.
- `unrestricted_permissions = true` now launches OpenCode with `--auto` (approve everything not explicitly denied in `opencode.json`). Previously the setting was a no-op for OpenCode. Its default remains ask-for-permission — unlike Claude's `auto`, OpenCode's `--auto` performs no model analysis and never falls back to a prompt.

### Quality of Life

- `foundry diff` streams git's output instead of buffering the whole patch, so large diffs start printing immediately instead of after the entire patch has been read, and render in color when attached to a terminal. Piped output is unchanged.
- `foundry status` is faster: it probes workspaces concurrently rather than one at a time, detects each project's main branch once per repo instead of once per workspace, and no longer runs the same `rev-list --count` twice per row.
- `foundry history` reads the tail of the log rather than parsing every event ever recorded. Showing the last 20 entries now costs the same on a 200,000-line log as on a short one; previously it deserialized the whole file.
- `open --all` no longer pauses between workspaces on tmux, Zellij, WezTerm, or the bare fallback. Those backends block until the workspace is up, so the 500 ms settle delay now applies only to Ghostty, iTerm2, and Windows Terminal, whose launches can return before the tab is ready.

### Fixed

- `foundry status` no longer panicked on non-ASCII agent output. The dashboard truncated the agent's last tool and last message at fixed byte offsets, so a multi-byte character straddling the cut aborted the command — and took `--watch` down with it.
- Each workspace now gets its own agent-status directory (`~/.foundry/status/<project>/<workspace>/<agent>.json`). The previous flat `<workspace>-<agent>.json` layout was ambiguous when one workspace name was a prefix of another: `foundry status` invented a phantom agent for the shorter name, and finishing or discarding it deleted the *neighbour's* live status. Workspaces created before this keep working — the old files are still read and cleaned up.
- `foundry discard` can now clean up a workspace whose worktree directory was removed out of band. It previously refused, leaving the git worktree registration and the branch stranded with no way to clear them; it now prunes the stale registration and archives or deletes the branch as usual. Relatedly, no command silently drops such an entry from `state.toml` any more — `list`, `status` and `open` all used to persist that pruning, erasing the record `discard` needs. `foundry open` with no arguments now lists a stale workspace with a `[missing]` marker instead of hiding it. A stale entry stays in `state.toml`, holding its port reservations, until discarded.
- `foundry start` run from inside a workspace tab no longer registers that workspace's worktree as a new project. It resolved the current repo with `--show-toplevel`, which inside a linked worktree reports the worktree itself; it now resolves back to the source repo, matching how the trust store already identified projects.
- Commands no longer rebuild a workspace's worktree path from `worktree_dir`. They read the path recorded when the workspace was created, so changing that setting no longer makes every existing workspace unreachable — including by `discard`, which was the only way back.
- Registered projects are matched by canonical path, so a repo reached through a symlink (or `/var` vs `/private/var` on macOS) is recognised instead of failing with "already registered to a different path".
- Deferred setup scripts are no longer typed into a running agent's pane. On Ghostty, iTerm2, WezTerm and Windows Terminal, a pane marked `deferred` that also ran an agent was selected as the target for the deferred scripts without having its own command suppressed, so the agent launched and the scripts were sent to its prompt.
- `foundry restore` runs setup scripts with the workspace's allocated ports, matching `start`. It also no longer runs scripts marked `deferred`, which are meant for a pane and would block the command indefinitely; it reports them instead.
- Broken pane layouts are rejected with a clear error before any branch or worktree is created, rather than failing differently in each terminal backend — Zellij previously overflowed its stack on a `split_from` cycle, and Ghostty silently merged two pane names that differed only in punctuation.
- `foundry discard` no longer reports success before doing the work. The message was printed ahead of the teardown scripts, the worktree removal and the branch operation, so any failure printed "Discarded workspace…" followed by an error with nothing cleaned up. It also no longer leaves the workspace's context file behind, and records the branch it actually archived rather than a recomputed name that could differ.
- `foundry checks` works when the worktree directory is missing. It only needs the PR number and branch from state, but had come to require the directory.
- `foundry status --watch` now reloads `state.toml` on each refresh, so workspaces started or finished in another tab appear and disappear live. Previously the dashboard stayed frozen on the snapshot taken when it launched, while still re-running every git command each tick.
- Read-only `git status` calls now pass `--no-optional-locks`, so foundry's status polling no longer rewrites the index or contends for `.git/index.lock` with agents running git in the same worktree. `status --watch` did this against every worktree every 2 seconds.
- Piping foundry's output into a command that stops reading early — `foundry diff | head`, or quitting a pager — no longer prints a "failed printing to stdout: Broken pipe" panic. Foundry now exits quietly on `SIGPIPE` the way other Unix tools in a pipeline do.
- Workspace state and project registry writes are now atomic (write to a temp file, then rename). A plain truncate-then-write left a window where a concurrent reader — notably `status --watch` — could parse a half-written file as "no workspaces" rather than failing, and two commands saving at once could interleave.

- Ghostty: deferred setup scripts now run, and `finish`/`discard` now close the workspace tab. Both operations located the tab by matching a terminal's `working directory` against the worktree path, but Ghostty never populates that property (empty on 1.3.1 even with shell integration active), so the lookup silently matched nothing. Foundry now records the tab id when it creates the tab and addresses the tab by id. Note that Ghostty's `confirm-close-surface` (on by default) prompts before closing a tab whose panes still have running processes.

## v0.5.0

### New Agent Support

- Pi (`pi`) — minimal terminal coding agent ([pi.dev](https://pi.dev/)). Positional prompt passing and `--continue` for session resume. Pi has no permission popups by design and offers no auto-approve flag, so `unrestricted_permissions` has no effect.

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
