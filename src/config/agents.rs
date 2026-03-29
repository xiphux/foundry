use anyhow::{Context, Result};

/// Per-agent capabilities. Groups all agent-specific knowledge in one place
/// so adding a new agent is a single registry entry rather than touching
/// multiple match arms across the codebase.
pub struct AgentCapabilities {
    /// All executable names this agent might be invoked as in a pane command.
    /// Used by `warn_agent_in_command` to detect when users should use the
    /// `agent` field instead of `command`.
    pub names: &'static [&'static str],
    /// The primary executable to check for on $PATH (e.g., "claude", "kiro-cli").
    pub executable: &'static str,
    /// Build the full command string for this agent.
    /// Each agent knows its own executable, flags, and how to incorporate
    /// the prompt, resume, and permission parameters.
    ///
    /// When `unrestricted` is false (default), agents that support sandboxed/scoped
    /// permissions use those for worktree-isolated operation. Agents without
    /// sandboxing launch with standard permissions (the user approves actions).
    ///
    /// When `unrestricted` is true, agents use their most permissive mode
    /// (e.g., YOLO/auto-approve-all), bypassing all permission prompts.
    ///
    /// `system_context` is an optional string appended to the agent's system
    /// prompt (e.g., workspace port info). Only agents with a CLI flag for
    /// this (like Claude's `--append-system-prompt`) will use it.
    pub build_command: fn(
        prompt: Option<&str>,
        resume: bool,
        unrestricted: bool,
        system_context: Option<&str>,
    ) -> String,
}

/// Escape a prompt string for use in a shell single-quoted argument.
fn escape_prompt(prompt: &str) -> String {
    prompt.replace('\'', "'\\''")
}

/// Registry of known agents and their capabilities.
const AGENT_REGISTRY: &[(&str, AgentCapabilities)] = &[
    (
        "claude",
        AgentCapabilities {
            names: &["claude"],
            executable: "claude",
            // Default: Claude uses sandbox + settings.local.json for worktree-scoped
            // permissions (level #2) with acceptEdits mode so file edits don't prompt.
            // Unrestricted: no sandbox constraints, still acceptEdits.
            // Note: sandbox is configured separately in agent_hooks.rs via settings.
            build_command: |prompt, resume, unrestricted, system_context| {
                let mut cmd = if unrestricted {
                    "claude --permission-mode bypassPermissions".to_string()
                } else {
                    "claude --permission-mode acceptEdits".to_string()
                };
                if resume {
                    cmd += " --continue";
                }
                if let Some(ctx) = system_context {
                    cmd += &format!(" --append-system-prompt '{}'", escape_prompt(ctx));
                }
                if let Some(p) = prompt {
                    cmd += &format!(" '{}'", escape_prompt(p));
                }
                cmd
            },
        },
    ),
    (
        "codex",
        AgentCapabilities {
            names: &["codex"],
            executable: "codex",
            // Codex has a built-in OS sandbox with --full-auto. The sandbox is
            // always active (level #2). unrestricted doesn't change behavior.
            build_command: |prompt, resume, _unrestricted, _system_context| {
                let mut cmd = "codex --full-auto".to_string();
                if resume {
                    cmd += " --resume";
                }
                if let Some(p) = prompt {
                    cmd += &format!(" '{}'", escape_prompt(p));
                }
                cmd
            },
        },
    ),
    (
        "every-code",
        AgentCapabilities {
            names: &["coder", "every-code"],
            executable: "coder",
            // Same sandbox model as Codex.
            build_command: |prompt, resume, _unrestricted, _system_context| {
                let mut cmd = "coder --full-auto".to_string();
                if resume {
                    cmd += " --resume";
                }
                if let Some(p) = prompt {
                    cmd += &format!(" '{}'", escape_prompt(p));
                }
                cmd
            },
        },
    ),
    (
        "gemini",
        AgentCapabilities {
            names: &["gemini"],
            executable: "gemini",
            // Default: sandbox mode restricts writes to project directory (level #2).
            // Unrestricted: yolo mode without sandbox (level #3).
            build_command: |prompt, resume, unrestricted, _system_context| {
                let mut cmd = if unrestricted {
                    "gemini --approval-mode=yolo".to_string()
                } else {
                    "gemini --sandbox --approval-mode=yolo".to_string()
                };
                if resume {
                    cmd += " --resume";
                }
                if let Some(p) = prompt {
                    cmd += &format!(" -p '{}'", escape_prompt(p));
                }
                cmd
            },
        },
    ),
    (
        "aider",
        AgentCapabilities {
            names: &["aider"],
            executable: "aider",
            // Default: interactive REPL, user approves actions (level #1).
            // Unrestricted: --yes auto-approves all confirmations (level #3).
            // Never passes --message (which would auto-exit after processing).
            build_command: |_prompt, _resume, unrestricted, _system_context| {
                if unrestricted {
                    "aider --yes".to_string()
                } else {
                    "aider".to_string()
                }
            },
        },
    ),
    (
        "copilot",
        AgentCapabilities {
            names: &["copilot"],
            executable: "copilot",
            // Default: standard permissions, user approves actions (level #1).
            // Unrestricted: --yolo enables all permissions (level #3).
            build_command: |prompt, _resume, unrestricted, _system_context| {
                let mut cmd = if unrestricted {
                    "copilot --yolo".to_string()
                } else {
                    "copilot".to_string()
                };
                if let Some(p) = prompt {
                    cmd += &format!(" -p '{}'", escape_prompt(p));
                }
                cmd
            },
        },
    ),
    (
        "kiro",
        AgentCapabilities {
            names: &["kiro", "kiro-cli"],
            executable: "kiro-cli",
            // Default: standard permissions, user approves tool usage (level #1).
            // Unrestricted: --trust-all-tools auto-approves (level #3).
            build_command: |prompt, resume, unrestricted, _system_context| {
                let mut cmd = "kiro-cli chat".to_string();
                if unrestricted {
                    cmd += " --trust-all-tools";
                }
                if resume {
                    cmd += " --resume";
                }
                if let Some(p) = prompt {
                    cmd += &format!(" '{}'", escape_prompt(p));
                }
                cmd
            },
        },
    ),
    (
        "opencode",
        AgentCapabilities {
            names: &["opencode"],
            executable: "opencode",
            // Default: interactive TUI, standard permissions (level #1).
            // Unrestricted: no CLI flag available; permissions must be configured
            // via opencode.json ("permission": "allow"). Flag is a no-op.
            build_command: |prompt, resume, _unrestricted, _system_context| {
                let mut cmd = "opencode".to_string();
                if resume {
                    cmd += " --continue";
                }
                if let Some(p) = prompt {
                    cmd += &format!(" --prompt '{}'", escape_prompt(p));
                }
                cmd
            },
        },
    ),
];

/// Look up capabilities for a known agent. Returns None for unknown/custom agents.
pub fn agent_capabilities(agent: &str) -> Option<&'static AgentCapabilities> {
    AGENT_REGISTRY
        .iter()
        .find(|(id, _)| *id == agent)
        .map(|(_, caps)| caps)
}

/// Check that the executable for a known agent is available on $PATH.
/// Returns Ok for unknown/custom agents (no executable to check).
pub fn check_agent_available(agent: &str) -> Result<()> {
    if agent == "custom" {
        return Ok(());
    }
    if let Some(caps) = agent_capabilities(agent) {
        which::which(caps.executable).with_context(|| {
            format!(
                "'{}' is required for agent '{agent}' but was not found on your PATH. \
                 Install it or change the agent in your foundry config.",
                caps.executable
            )
        })?;
    }
    Ok(())
}

/// Build the full agent command string for a given agent identifier,
/// optionally including a prompt and/or session resume flag.
pub fn build_agent_command(
    agent: &str,
    custom_command: Option<&str>,
    prompt: Option<&str>,
    continue_session: bool,
    unrestricted: bool,
    system_context: Option<&str>,
) -> String {
    if agent == "custom" {
        return custom_command.unwrap_or("claude").to_string();
    }
    let non_empty_prompt = prompt.filter(|p| !p.is_empty());
    let non_empty_context = system_context.filter(|c| !c.is_empty());
    match agent_capabilities(agent) {
        Some(caps) => (caps.build_command)(
            non_empty_prompt,
            continue_session,
            unrestricted,
            non_empty_context,
        ),
        None => agent.to_string(),
    }
}

/// Resolve the base agent command from the agent identifier.
/// For known agents, returns the command without prompt or resume flags.
pub fn resolve_agent_command(agent: &str, custom_command: Option<&str>) -> String {
    build_agent_command(agent, custom_command, None, false, false, None)
}

/// Check if a pane command looks like it's invoking a known agent, and warn
/// that the `agent` field should be used instead.
pub fn warn_agent_in_command(panes: &[super::PaneConfig]) {
    for pane in panes {
        if pane.agent.is_some() {
            continue;
        }
        if let Some(ref cmd) = pane.command {
            let first_word = cmd.split_whitespace().next().unwrap_or("");
            for (agent_id, caps) in AGENT_REGISTRY {
                if first_word == *agent_id || caps.names.contains(&first_word) {
                    eprintln!(
                        "Warning: pane '{}' uses command '{cmd}' which looks like a known agent. \
                         Use `agent = \"{agent_id}\"` instead of `command` so foundry can set up \
                         permissions, status tracking, and prompt passthrough.",
                        pane.name
                    );
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_registry_has_known_agents() {
        assert!(agent_capabilities("claude").is_some());
        assert!(agent_capabilities("codex").is_some());
        assert!(agent_capabilities("every-code").is_some());
        assert!(agent_capabilities("gemini").is_some());
        assert!(agent_capabilities("aider").is_some());
        assert!(agent_capabilities("copilot").is_some());
        assert!(agent_capabilities("kiro").is_some());
        assert!(agent_capabilities("opencode").is_some());
        assert!(agent_capabilities("unknown").is_none());
    }

    #[test]
    fn agent_build_command_claude() {
        let caps = agent_capabilities("claude").unwrap();
        assert_eq!(
            (caps.build_command)(None, false, false, None),
            "claude --permission-mode acceptEdits"
        );
        assert_eq!(
            (caps.build_command)(None, true, false, None),
            "claude --permission-mode acceptEdits --continue"
        );
        assert_eq!(
            (caps.build_command)(Some("fix the bug"), false, false, None),
            "claude --permission-mode acceptEdits 'fix the bug'"
        );
        assert_eq!(
            (caps.build_command)(None, false, true, None),
            "claude --permission-mode bypassPermissions"
        );
        // System context appended
        assert_eq!(
            (caps.build_command)(None, false, false, Some("Dev server at :10042")),
            "claude --permission-mode acceptEdits --append-system-prompt 'Dev server at :10042'"
        );
        // System context + prompt (context before prompt)
        assert_eq!(
            (caps.build_command)(Some("fix it"), false, false, Some("port 10042")),
            "claude --permission-mode acceptEdits --append-system-prompt 'port 10042' 'fix it'"
        );
    }

    #[test]
    fn agent_build_command_codex() {
        let caps = agent_capabilities("codex").unwrap();
        let cmd = (caps.build_command)(None, false, false, None);
        assert!(cmd.starts_with("codex "));
        assert!(cmd.contains("--full-auto"));
        let cmd_resume = (caps.build_command)(None, true, false, None);
        assert!(cmd_resume.contains("--resume"));
    }

    #[test]
    fn agent_build_command_every_code() {
        let caps = agent_capabilities("every-code").unwrap();
        let cmd = (caps.build_command)(None, false, false, None);
        assert!(cmd.starts_with("coder "));
        assert!(cmd.contains("--full-auto"));
    }

    #[test]
    fn agent_build_command_gemini() {
        let caps = agent_capabilities("gemini").unwrap();
        assert_eq!(
            (caps.build_command)(None, false, false, None),
            "gemini --sandbox --approval-mode=yolo"
        );
        assert_eq!(
            (caps.build_command)(None, true, false, None),
            "gemini --sandbox --approval-mode=yolo --resume"
        );
        assert_eq!(
            (caps.build_command)(Some("fix the bug"), false, false, None),
            "gemini --sandbox --approval-mode=yolo -p 'fix the bug'"
        );
        assert_eq!(
            (caps.build_command)(None, false, true, None),
            "gemini --approval-mode=yolo"
        );
    }

    #[test]
    fn agent_build_command_aider() {
        let caps = agent_capabilities("aider").unwrap();
        assert_eq!((caps.build_command)(None, false, false, None), "aider");
        assert_eq!((caps.build_command)(None, false, true, None), "aider --yes");
    }

    #[test]
    fn agent_build_command_copilot() {
        let caps = agent_capabilities("copilot").unwrap();
        assert_eq!((caps.build_command)(None, false, false, None), "copilot");
        assert_eq!(
            (caps.build_command)(Some("fix the bug"), false, false, None),
            "copilot -p 'fix the bug'"
        );
        assert_eq!(
            (caps.build_command)(None, false, true, None),
            "copilot --yolo"
        );
    }

    #[test]
    fn agent_build_command_kiro() {
        let caps = agent_capabilities("kiro").unwrap();
        assert_eq!(
            (caps.build_command)(None, false, false, None),
            "kiro-cli chat"
        );
        assert_eq!(
            (caps.build_command)(None, true, false, None),
            "kiro-cli chat --resume"
        );
        assert_eq!(
            (caps.build_command)(None, false, true, None),
            "kiro-cli chat --trust-all-tools"
        );
        assert_eq!(
            (caps.build_command)(Some("fix the bug"), false, true, None),
            "kiro-cli chat --trust-all-tools 'fix the bug'"
        );
    }

    #[test]
    fn agent_build_command_opencode() {
        let caps = agent_capabilities("opencode").unwrap();
        assert_eq!((caps.build_command)(None, false, false, None), "opencode");
        assert_eq!(
            (caps.build_command)(None, true, false, None),
            "opencode --continue"
        );
        assert_eq!(
            (caps.build_command)(Some("fix the bug"), false, false, None),
            "opencode --prompt 'fix the bug'"
        );
    }

    #[test]
    fn agent_names_detection() {
        let caps = agent_capabilities("claude").unwrap();
        assert!(caps.names.contains(&"claude"));

        let caps = agent_capabilities("every-code").unwrap();
        assert!(caps.names.contains(&"coder"));
        assert!(caps.names.contains(&"every-code"));
    }

    #[test]
    fn resolve_agent_command_claude() {
        let cmd = resolve_agent_command("claude", None);
        assert!(cmd.starts_with("claude"));
    }

    #[test]
    fn resolve_agent_command_codex() {
        let cmd = resolve_agent_command("codex", None);
        assert!(cmd.starts_with("codex "));
        assert!(cmd.contains("--full-auto"));
    }

    #[test]
    fn resolve_agent_command_every_code() {
        let cmd = resolve_agent_command("every-code", None);
        assert!(cmd.starts_with("coder "));
        assert!(cmd.contains("--full-auto"));
    }

    #[test]
    fn resolve_agent_command_custom_with_command() {
        assert_eq!(
            resolve_agent_command("custom", Some("my-agent --flag")),
            "my-agent --flag"
        );
    }

    #[test]
    fn resolve_agent_command_custom_without_command_defaults_to_claude() {
        assert_eq!(resolve_agent_command("custom", None), "claude");
    }

    #[test]
    fn resolve_agent_command_unknown_passthrough() {
        assert_eq!(
            resolve_agent_command("some-other-agent", None),
            "some-other-agent"
        );
    }

    #[test]
    fn warn_agent_in_command_detects_agent_commands() {
        let panes = vec![super::super::PaneConfig {
            name: "test".into(),
            agent: None,
            command: Some("claude --dangerously-skip-permissions".into()),
            split_from: None,
            direction: None,
            optional: false,
            env: Default::default(),
            deferred: false,
        }];
        warn_agent_in_command(&panes);
    }

    #[test]
    fn warn_agent_in_command_skips_agent_panes() {
        let panes = vec![super::super::PaneConfig {
            name: "test".into(),
            agent: Some("claude".into()),
            command: Some("claude".into()),
            split_from: None,
            direction: None,
            optional: false,
            env: Default::default(),
            deferred: false,
        }];
        warn_agent_in_command(&panes);
    }
}
