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
    ///
    /// Each agent knows its own executable and flags, and decides what every
    /// field of the invocation means for it — including the ones it cannot
    /// honour, which it simply ignores. That is the whole contract: a mode this
    /// registry cannot express is a mode foundry cannot ask for.
    pub build_command: fn(&AgentInvocation) -> String,
}

/// Everything that varies between one launch of an agent and the next.
///
/// A struct rather than positional parameters because the alternative did not
/// hold. `plan` arrived as a post-hoc rewrite of the string an agent had
/// already produced:
///
/// ```ignore
/// if plan && !unrestricted && cmd.contains("--permission-mode auto") {
///     cmd = cmd.replace("--permission-mode auto", "--permission-mode plan");
/// }
/// ```
///
/// which silently stopped working the moment Claude's entry spelled its flag
/// differently — no error, no failing test, just a plan-mode session that was
/// not in plan mode. Passing the intent to the agent instead of pattern-matching
/// its output keeps that knowledge in the one place that owns it.
#[derive(Debug, Clone, Copy, Default)]
pub struct AgentInvocation<'a> {
    /// Initial prompt, if the agent takes one. Agents whose non-interactive
    /// mode would exit after processing it ignore this.
    pub prompt: Option<&'a str>,
    /// Resume the previous conversation in this worktree.
    pub resume: bool,
    /// Use the agent's most permissive mode, bypassing all permission prompts.
    /// Agents that are always permissive, or offer no such flag, ignore this.
    pub unrestricted: bool,
    /// Require plan approval before any edits. Only Claude implements it;
    /// `unrestricted` takes precedence where both are set.
    pub plan: bool,
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
            // Default: `auto` mode, where Claude uses model analysis to approve
            // most permission prompts, combined with settings.local.json for
            // worktree-scoped allow/deny rules (level #2).
            // Unrestricted: bypass permission checks entirely.
            // Plan: require plan approval before any edits. Checked after
            // unrestricted, which wins where a user has asked for both.
            build_command: |inv| {
                let mode = if inv.unrestricted {
                    "bypassPermissions"
                } else if inv.plan {
                    "plan"
                } else {
                    "auto"
                };
                let mut cmd = format!("claude --permission-mode {mode}");
                if inv.resume {
                    cmd += " --continue";
                }
                if let Some(p) = inv.prompt {
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
            build_command: |inv| {
                let mut cmd = "codex --full-auto".to_string();
                if inv.resume {
                    cmd += " --resume";
                }
                if let Some(p) = inv.prompt {
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
            build_command: |inv| {
                let mut cmd = "coder --full-auto".to_string();
                if inv.resume {
                    cmd += " --resume";
                }
                if let Some(p) = inv.prompt {
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
            build_command: |inv| {
                let mut cmd = if inv.unrestricted {
                    "gemini --approval-mode=yolo".to_string()
                } else {
                    "gemini --sandbox --approval-mode=yolo".to_string()
                };
                if inv.resume {
                    cmd += " --resume";
                }
                if let Some(p) = inv.prompt {
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
            build_command: |inv| {
                if inv.unrestricted {
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
            build_command: |inv| {
                let mut cmd = if inv.unrestricted {
                    "copilot --yolo".to_string()
                } else {
                    "copilot".to_string()
                };
                if let Some(p) = inv.prompt {
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
            build_command: |inv| {
                let mut cmd = "kiro-cli chat".to_string();
                if inv.unrestricted {
                    cmd += " --trust-all-tools";
                }
                if inv.resume {
                    cmd += " --resume";
                }
                if let Some(p) = inv.prompt {
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
            // Unrestricted: --auto approves every permission not explicitly
            // denied in opencode.json (level #3). Unlike Claude's `auto` mode,
            // there is no model analysis and no fallback prompt.
            build_command: |inv| {
                let mut cmd = if inv.unrestricted {
                    "opencode --auto".to_string()
                } else {
                    "opencode".to_string()
                };
                if inv.resume {
                    cmd += " --continue";
                }
                if let Some(p) = inv.prompt {
                    cmd += &format!(" --prompt '{}'", escape_prompt(p));
                }
                cmd
            },
        },
    ),
    (
        "crush",
        AgentCapabilities {
            names: &["crush"],
            executable: "crush",
            // Default: interactive TUI, user approves tool calls (level #1).
            // Unrestricted: --yolo skips all permission prompts (level #3).
            // No prompt passthrough in interactive mode (crush run exits after prompt).
            build_command: |inv| {
                let mut cmd = if inv.unrestricted {
                    "crush --yolo".to_string()
                } else {
                    "crush".to_string()
                };
                if inv.resume {
                    cmd += " --continue";
                }
                cmd
            },
        },
    ),
    (
        "nanocoder",
        AgentCapabilities {
            names: &["nanocoder"],
            executable: "nanocoder",
            // Default: interactive TUI, user approves tool calls (level #1).
            // No CLI auto-approve flag available.
            // Prompt is passed as a positional argument.
            build_command: |inv| {
                let mut cmd = "nanocoder".to_string();
                if let Some(p) = inv.prompt {
                    cmd += &format!(" '{}'", escape_prompt(p));
                }
                cmd
            },
        },
    ),
    (
        "pi",
        AgentCapabilities {
            names: &["pi"],
            executable: "pi",
            // Pi has no permission popups by design — it executes tools without
            // approval prompts and offers no auto-approve/restrict flag, so the
            // unrestricted flag is a no-op (Pi is always permissive). Pi's docs
            // recommend running it in a container for isolation.
            // Prompt is passed as a positional argument; --continue resumes.
            build_command: |inv| {
                let mut cmd = "pi".to_string();
                if inv.resume {
                    cmd += " --continue";
                }
                if let Some(p) = inv.prompt {
                    cmd += &format!(" '{}'", escape_prompt(p));
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

/// Build the shell command that launches `agent` for this invocation.
///
/// An unregistered identifier is returned verbatim, which is how a user names
/// an agent foundry has never heard of. `trust.rs` knows this and gates it,
/// since it means `.foundry.toml` can name an arbitrary command that way.
pub fn build_agent_command(
    agent: &str,
    custom_command: Option<&str>,
    invocation: &AgentInvocation,
) -> String {
    if agent == "custom" {
        return custom_command.unwrap_or("claude").to_string();
    }
    let Some(caps) = agent_capabilities(agent) else {
        return agent.to_string();
    };
    // An empty prompt is not a prompt — passing it through would leave a bare
    // `''` argument on the command line.
    let invocation = AgentInvocation {
        prompt: invocation.prompt.filter(|p| !p.is_empty()),
        ..*invocation
    };
    (caps.build_command)(&invocation)
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
        assert!(agent_capabilities("crush").is_some());
        assert!(agent_capabilities("nanocoder").is_some());
        assert!(agent_capabilities("pi").is_some());
        assert!(agent_capabilities("unknown").is_none());
    }

    #[test]
    fn agent_build_command_claude() {
        let caps = agent_capabilities("claude").unwrap();
        assert_eq!(
            (caps.build_command)(&AgentInvocation::default()),
            "claude --permission-mode auto"
        );
        assert_eq!(
            (caps.build_command)(&AgentInvocation {
                resume: true,
                ..Default::default()
            }),
            "claude --permission-mode auto --continue"
        );
        assert_eq!(
            (caps.build_command)(&AgentInvocation {
                prompt: Some("fix the bug"),
                ..Default::default()
            }),
            "claude --permission-mode auto 'fix the bug'"
        );
        assert_eq!(
            (caps.build_command)(&AgentInvocation {
                unrestricted: true,
                ..Default::default()
            }),
            "claude --permission-mode bypassPermissions"
        );
    }

    #[test]
    fn agent_build_command_claude_plan_mode() {
        let cmd = build_agent_command(
            "claude",
            None,
            &AgentInvocation {
                prompt: Some("plan this"),
                plan: true,
                ..Default::default()
            },
        );
        assert!(cmd.contains("--permission-mode plan"));
        assert!(!cmd.contains("--permission-mode auto"));
        assert!(cmd.contains("'plan this'"));
    }

    /// Plan mode used to be applied by rewriting the string Claude's entry had
    /// already produced, keyed on the literal `--permission-mode auto`. That
    /// coupling was invisible: respelling the flag would have left `--plan`
    /// silently doing nothing. Assert the mode is chosen, not patched.
    #[test]
    fn plan_mode_is_chosen_by_the_agent_not_patched_into_its_output() {
        let caps = agent_capabilities("claude").unwrap();
        let plain = (caps.build_command)(&AgentInvocation::default());
        let planned = (caps.build_command)(&AgentInvocation {
            plan: true,
            ..Default::default()
        });

        assert_eq!(plain, "claude --permission-mode auto");
        assert_eq!(planned, "claude --permission-mode plan");
        // Nothing but the mode differs — plan is not an extra flag bolted on.
        assert_eq!(plain.replace("auto", "plan"), planned);
    }

    /// Every other agent must tolerate the flag rather than react to it.
    #[test]
    fn plan_mode_is_inert_for_agents_that_do_not_support_it() {
        for id in [
            "codex",
            "every-code",
            "gemini",
            "aider",
            "copilot",
            "kiro",
            "opencode",
            "crush",
            "nanocoder",
            "pi",
        ] {
            let caps = agent_capabilities(id).unwrap();
            assert_eq!(
                (caps.build_command)(&AgentInvocation::default()),
                (caps.build_command)(&AgentInvocation {
                    plan: true,
                    ..Default::default()
                }),
                "{id} changed its command for plan mode"
            );
        }
    }

    #[test]
    fn agent_build_command_plan_mode_ignored_when_unrestricted() {
        // unrestricted takes precedence — plan flag is ignored
        let cmd = build_agent_command(
            "claude",
            None,
            &AgentInvocation {
                unrestricted: true,
                plan: true,
                ..Default::default()
            },
        );
        assert!(cmd.contains("--permission-mode bypassPermissions"));
        assert!(!cmd.contains("plan"));
    }

    #[test]
    fn agent_build_command_plan_mode_ignored_for_non_claude() {
        let cmd = build_agent_command(
            "codex",
            None,
            &AgentInvocation {
                plan: true,
                ..Default::default()
            },
        );
        assert!(cmd.contains("--full-auto"));
        assert!(!cmd.contains("plan"));
    }

    #[test]
    fn agent_build_command_codex() {
        let caps = agent_capabilities("codex").unwrap();
        let cmd = (caps.build_command)(&AgentInvocation::default());
        assert!(cmd.starts_with("codex "));
        assert!(cmd.contains("--full-auto"));
        let cmd_resume = (caps.build_command)(&AgentInvocation {
            resume: true,
            ..Default::default()
        });
        assert!(cmd_resume.contains("--resume"));
    }

    #[test]
    fn agent_build_command_every_code() {
        let caps = agent_capabilities("every-code").unwrap();
        let cmd = (caps.build_command)(&AgentInvocation::default());
        assert!(cmd.starts_with("coder "));
        assert!(cmd.contains("--full-auto"));
    }

    #[test]
    fn agent_build_command_gemini() {
        let caps = agent_capabilities("gemini").unwrap();
        assert_eq!(
            (caps.build_command)(&AgentInvocation::default()),
            "gemini --sandbox --approval-mode=yolo"
        );
        assert_eq!(
            (caps.build_command)(&AgentInvocation {
                resume: true,
                ..Default::default()
            }),
            "gemini --sandbox --approval-mode=yolo --resume"
        );
        assert_eq!(
            (caps.build_command)(&AgentInvocation {
                prompt: Some("fix the bug"),
                ..Default::default()
            }),
            "gemini --sandbox --approval-mode=yolo -p 'fix the bug'"
        );
        assert_eq!(
            (caps.build_command)(&AgentInvocation {
                unrestricted: true,
                ..Default::default()
            }),
            "gemini --approval-mode=yolo"
        );
    }

    #[test]
    fn agent_build_command_aider() {
        let caps = agent_capabilities("aider").unwrap();
        assert_eq!((caps.build_command)(&AgentInvocation::default()), "aider");
        assert_eq!(
            (caps.build_command)(&AgentInvocation {
                unrestricted: true,
                ..Default::default()
            }),
            "aider --yes"
        );
    }

    #[test]
    fn agent_build_command_copilot() {
        let caps = agent_capabilities("copilot").unwrap();
        assert_eq!((caps.build_command)(&AgentInvocation::default()), "copilot");
        assert_eq!(
            (caps.build_command)(&AgentInvocation {
                prompt: Some("fix the bug"),
                ..Default::default()
            }),
            "copilot -p 'fix the bug'"
        );
        assert_eq!(
            (caps.build_command)(&AgentInvocation {
                unrestricted: true,
                ..Default::default()
            }),
            "copilot --yolo"
        );
    }

    #[test]
    fn agent_build_command_kiro() {
        let caps = agent_capabilities("kiro").unwrap();
        assert_eq!(
            (caps.build_command)(&AgentInvocation::default()),
            "kiro-cli chat"
        );
        assert_eq!(
            (caps.build_command)(&AgentInvocation {
                resume: true,
                ..Default::default()
            }),
            "kiro-cli chat --resume"
        );
        assert_eq!(
            (caps.build_command)(&AgentInvocation {
                unrestricted: true,
                ..Default::default()
            }),
            "kiro-cli chat --trust-all-tools"
        );
        assert_eq!(
            (caps.build_command)(&AgentInvocation {
                prompt: Some("fix the bug"),
                unrestricted: true,
                ..Default::default()
            }),
            "kiro-cli chat --trust-all-tools 'fix the bug'"
        );
    }

    #[test]
    fn agent_build_command_opencode() {
        let caps = agent_capabilities("opencode").unwrap();
        assert_eq!(
            (caps.build_command)(&AgentInvocation::default()),
            "opencode"
        );
        assert_eq!(
            (caps.build_command)(&AgentInvocation {
                resume: true,
                ..Default::default()
            }),
            "opencode --continue"
        );
        assert_eq!(
            (caps.build_command)(&AgentInvocation {
                prompt: Some("fix the bug"),
                ..Default::default()
            }),
            "opencode --prompt 'fix the bug'"
        );
        assert_eq!(
            (caps.build_command)(&AgentInvocation {
                unrestricted: true,
                ..Default::default()
            }),
            "opencode --auto"
        );
        assert_eq!(
            (caps.build_command)(&AgentInvocation {
                prompt: Some("fix the bug"),
                resume: true,
                unrestricted: true,
                ..Default::default()
            }),
            "opencode --auto --continue --prompt 'fix the bug'"
        );
    }

    #[test]
    fn agent_build_command_crush() {
        let caps = agent_capabilities("crush").unwrap();
        // Default: interactive TUI, no auto-approve
        assert_eq!((caps.build_command)(&AgentInvocation::default()), "crush");
        // Resume
        assert_eq!(
            (caps.build_command)(&AgentInvocation {
                resume: true,
                ..Default::default()
            }),
            "crush --continue"
        );
        // Unrestricted: --yolo
        assert_eq!(
            (caps.build_command)(&AgentInvocation {
                unrestricted: true,
                ..Default::default()
            }),
            "crush --yolo"
        );
        // Prompt ignored (no interactive prompt passthrough)
        assert_eq!(
            (caps.build_command)(&AgentInvocation {
                prompt: Some("fix the bug"),
                ..Default::default()
            }),
            "crush"
        );
    }

    #[test]
    fn agent_build_command_nanocoder() {
        let caps = agent_capabilities("nanocoder").unwrap();
        assert_eq!(
            (caps.build_command)(&AgentInvocation::default()),
            "nanocoder"
        );
        assert_eq!(
            (caps.build_command)(&AgentInvocation {
                prompt: Some("fix the bug"),
                ..Default::default()
            }),
            "nanocoder 'fix the bug'"
        );
        // No resume or unrestricted support
        assert_eq!(
            (caps.build_command)(&AgentInvocation {
                resume: true,
                ..Default::default()
            }),
            "nanocoder"
        );
        assert_eq!(
            (caps.build_command)(&AgentInvocation {
                unrestricted: true,
                ..Default::default()
            }),
            "nanocoder"
        );
    }

    #[test]
    fn agent_build_command_pi() {
        let caps = agent_capabilities("pi").unwrap();
        assert_eq!((caps.build_command)(&AgentInvocation::default()), "pi");
        assert_eq!(
            (caps.build_command)(&AgentInvocation {
                resume: true,
                ..Default::default()
            }),
            "pi --continue"
        );
        assert_eq!(
            (caps.build_command)(&AgentInvocation {
                prompt: Some("fix the bug"),
                ..Default::default()
            }),
            "pi 'fix the bug'"
        );
        // No auto-approve flag — unrestricted is a no-op
        assert_eq!(
            (caps.build_command)(&AgentInvocation {
                unrestricted: true,
                ..Default::default()
            }),
            "pi"
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
        let cmd = build_agent_command("claude", None, &AgentInvocation::default());
        assert!(cmd.starts_with("claude"));
    }

    #[test]
    fn resolve_agent_command_codex() {
        let cmd = build_agent_command("codex", None, &AgentInvocation::default());
        assert!(cmd.starts_with("codex "));
        assert!(cmd.contains("--full-auto"));
    }

    #[test]
    fn resolve_agent_command_every_code() {
        let cmd = build_agent_command("every-code", None, &AgentInvocation::default());
        assert!(cmd.starts_with("coder "));
        assert!(cmd.contains("--full-auto"));
    }

    #[test]
    fn resolve_agent_command_custom_with_command() {
        assert_eq!(
            build_agent_command(
                "custom",
                Some("my-agent --flag"),
                &AgentInvocation::default()
            ),
            "my-agent --flag"
        );
    }

    #[test]
    fn resolve_agent_command_custom_without_command_defaults_to_claude() {
        assert_eq!(
            build_agent_command("custom", None, &AgentInvocation::default()),
            "claude"
        );
    }

    #[test]
    fn resolve_agent_command_unknown_passthrough() {
        assert_eq!(
            build_agent_command("some-other-agent", None, &AgentInvocation::default()),
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
