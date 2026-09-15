use clap::{Parser, Subcommand};
use clap_complete::Shell;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "foundry", about = "AI agent workspace manager", version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Specify project explicitly
    #[arg(long, global = true)]
    pub project: Option<String>,

    /// Show detailed output
    #[arg(long, global = true)]
    pub verbose: bool,

    /// Skip confirmation prompts
    #[arg(long, global = true)]
    pub yes: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Create branch, worktree, run setup, open workspace
    #[command(visible_alias = "create")]
    Start {
        /// Name for the feature branch / worktree (auto-generated if --issue is used)
        name: Option<String>,

        /// Create workspace from a GitHub issue (number or URL)
        #[arg(long)]
        issue: Option<String>,

        /// Prompt to pass to the AI agent
        #[arg(long)]
        prompt: Option<String>,

        /// File containing a prompt to pass to the AI agent
        #[arg(long, conflicts_with = "prompt")]
        prompt_file: Option<PathBuf>,

        /// Fetch and fast-forward main from remote before branching
        #[arg(long)]
        fetch: bool,

        /// Start Claude in plan mode (requires plan approval before edits)
        #[arg(long)]
        plan: bool,

        /// Override the configured agent for this workspace
        #[arg(long)]
        agent: Option<String>,
    },
    /// Reopen workspace for an existing worktree
    Open {
        /// Worktree name (lists active worktrees if omitted)
        name: Option<String>,

        /// Reopen all active workspaces for the project
        #[arg(long)]
        all: bool,
    },
    /// Finish workspace: merge PR (if created) or merge locally, teardown, clean up
    #[command(visible_alias = "merge")]
    Finish {
        /// Worktree name (inferred from cwd if omitted)
        name: Option<String>,

        /// Force local merge, ignoring any associated PR
        #[arg(long)]
        local: bool,
    },
    /// Push branch and create a pull request
    Pr {
        /// Worktree name (inferred from cwd if omitted)
        name: Option<String>,

        /// PR title (auto-generated from branch name if omitted)
        #[arg(long)]
        title: Option<String>,

        /// PR body/description
        #[arg(long)]
        body: Option<String>,
    },
    /// Show CI check status for a workspace's PR
    Checks {
        /// Worktree name (inferred from cwd if omitted)
        name: Option<String>,
    },
    /// Teardown and delete worktree without merging
    #[command(visible_alias = "destroy")]
    Discard {
        /// Worktree name (inferred from cwd if omitted)
        name: Option<String>,

        /// Force discard even if the branch has unmerged commits
        #[arg(long, short)]
        force: bool,
    },
    /// Open workspace in your configured editor
    Edit {
        /// Worktree name (inferred from cwd if omitted)
        name: Option<String>,
    },
    /// Open workspace directory in the system file explorer
    Browse {
        /// Worktree name (inferred from cwd if omitted)
        name: Option<String>,
    },
    /// Show changes in a workspace vs main
    Diff {
        /// Worktree name (inferred from cwd if omitted)
        name: Option<String>,

        /// Show file stats instead of full diff
        #[arg(long)]
        stat: bool,
    },
    /// Switch to an existing workspace's terminal tab
    Switch {
        /// Worktree name (lists active worktrees if omitted)
        name: Option<String>,
    },
    /// Restore a workspace from an archived branch
    Restore {
        /// Archived branch name (lists archived branches if omitted)
        branch: Option<String>,
    },
    /// Review and approve a project's .foundry.toml
    ///
    /// Project configs are checked into the repository and can specify
    /// commands foundry will run. Approval is recorded per project and is
    /// requested again whenever the file changes.
    Trust {
        /// Path to the project root (defaults to the current repository)
        path: Option<PathBuf>,

        /// Withdraw a previous approval instead of granting one
        #[arg(long)]
        revoke: bool,
    },
    /// Manage project registry
    #[command(subcommand)]
    Projects(ProjectsCommands),
    /// Show workspace activity history
    History {
        /// Number of recent events to show (default: 20)
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// List all active workspaces across all projects
    List,
    /// Show status dashboard of all active workspaces
    Status {
        /// Continuously refresh the display (every 2 seconds)
        #[arg(long, short)]
        watch: bool,
    },
    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        shell: Shell,
    },
}

#[derive(Subcommand)]
pub enum ProjectsCommands {
    /// List registered projects
    List,
    /// Register a project
    Add {
        /// Project name
        name: String,
        /// Path to the project root
        path: PathBuf,
    },
    /// Unregister a project
    Remove {
        /// Project name
        name: String,
    },
}

/// The command line is foundry's contract with its users and their shell
/// scripts, and clap and clap_complete are the dependencies Dependabot can
/// update underneath it. These pin what each command accepts, so a release
/// that changes parsing fails here rather than in someone's terminal.
#[cfg(test)]
mod tests {
    use super::*;
    use clap::{CommandFactory, ValueEnum};

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(std::iter::once("foundry").chain(args.iter().copied()))
            .unwrap_or_else(|e| panic!("`foundry {}` should parse: {e}", args.join(" ")))
    }

    fn rejects(args: &[&str]) {
        assert!(
            Cli::try_parse_from(std::iter::once("foundry").chain(args.iter().copied())).is_err(),
            "`foundry {}` should be rejected",
            args.join(" ")
        );
    }

    /// clap's own consistency check: conflicting or duplicate argument
    /// definitions, bad defaults and the like only fail when parsed otherwise.
    #[test]
    fn command_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn start_accepts_every_flag() {
        let cli = parse(&[
            "start", "feat", "--issue", "42", "--prompt", "do it", "--fetch", "--plan", "--agent",
            "codex",
        ]);
        let Commands::Start {
            name,
            issue,
            prompt,
            prompt_file,
            fetch,
            plan,
            agent,
        } = cli.command
        else {
            panic!("expected start");
        };
        assert_eq!(name.as_deref(), Some("feat"));
        assert_eq!(issue.as_deref(), Some("42"));
        assert_eq!(prompt.as_deref(), Some("do it"));
        assert_eq!(prompt_file, None);
        assert!(fetch && plan);
        assert_eq!(agent.as_deref(), Some("codex"));
    }

    #[test]
    fn start_name_is_optional_and_prompt_sources_conflict() {
        assert!(matches!(
            parse(&["start", "--prompt-file", "p.md"]).command,
            Commands::Start {
                name: None,
                prompt_file: Some(_),
                ..
            }
        ));
        rejects(&["start", "x", "--prompt", "a", "--prompt-file", "p.md"]);
    }

    #[test]
    fn visible_aliases_resolve_to_their_commands() {
        assert!(matches!(
            parse(&["create", "x"]).command,
            Commands::Start { .. }
        ));
        assert!(matches!(
            parse(&["merge", "x"]).command,
            Commands::Finish { .. }
        ));
        assert!(matches!(
            parse(&["destroy", "x"]).command,
            Commands::Discard { .. }
        ));
    }

    #[test]
    fn global_flags_work_before_and_after_the_subcommand() {
        for args in [
            &["--project", "p", "--verbose", "--yes", "finish", "x"][..],
            &["finish", "x", "--project", "p", "--verbose", "--yes"][..],
        ] {
            let cli = parse(args);
            assert_eq!(cli.project.as_deref(), Some("p"));
            assert!(cli.verbose && cli.yes);
        }
    }

    #[test]
    fn workspace_commands_take_an_optional_name() {
        for command in [
            "open", "finish", "pr", "checks", "discard", "edit", "browse", "diff", "switch",
        ] {
            parse(&[command]);
            parse(&[command, "name"]);
        }
    }

    #[test]
    fn command_flags() {
        assert!(matches!(
            parse(&["open", "--all"]).command,
            Commands::Open {
                name: None,
                all: true
            }
        ));
        assert!(matches!(
            parse(&["finish", "x", "--local"]).command,
            Commands::Finish { local: true, .. }
        ));
        assert!(matches!(
            parse(&["pr", "x", "--title", "t", "--body", "b"]).command,
            Commands::Pr {
                title: Some(_),
                body: Some(_),
                ..
            }
        ));
        for flag in ["--force", "-f"] {
            assert!(matches!(
                parse(&["discard", "x", flag]).command,
                Commands::Discard { force: true, .. }
            ));
        }
        assert!(matches!(
            parse(&["diff", "x", "--stat"]).command,
            Commands::Diff { stat: true, .. }
        ));
        assert!(matches!(
            parse(&["restore", "archive/x-20260101"]).command,
            Commands::Restore { branch: Some(_) }
        ));
        assert!(matches!(
            parse(&["trust", "/repo", "--revoke"]).command,
            Commands::Trust {
                path: Some(_),
                revoke: true
            }
        ));
        for flag in ["--watch", "-w"] {
            assert!(matches!(
                parse(&["status", flag]).command,
                Commands::Status { watch: true }
            ));
        }
        assert!(matches!(parse(&["list"]).command, Commands::List));
    }

    #[test]
    fn history_limit_defaults_to_20_and_must_be_a_number() {
        assert!(matches!(
            parse(&["history"]).command,
            Commands::History { limit: 20 }
        ));
        assert!(matches!(
            parse(&["history", "--limit", "5"]).command,
            Commands::History { limit: 5 }
        ));
        rejects(&["history", "--limit", "many"]);
    }

    #[test]
    fn projects_subcommands() {
        assert!(matches!(
            parse(&["projects", "list"]).command,
            Commands::Projects(ProjectsCommands::List)
        ));
        assert!(matches!(
            parse(&["projects", "add", "app", "/src/app"]).command,
            Commands::Projects(ProjectsCommands::Add { .. })
        ));
        assert!(matches!(
            parse(&["projects", "remove", "app"]).command,
            Commands::Projects(ProjectsCommands::Remove { .. })
        ));
        rejects(&["projects", "add", "app"]);
        rejects(&["projects"]);
    }

    #[test]
    fn unknown_commands_and_flags_are_rejected() {
        rejects(&[]);
        rejects(&["frobnicate"]);
        rejects(&["finish", "x", "--no-such-flag"]);
    }

    /// Every shell clap_complete supports produces a script that knows every
    /// subcommand, so an update that drops a shell or breaks generation fails.
    #[test]
    fn completions_generate_for_every_shell() {
        let subcommands: Vec<String> = Cli::command()
            .get_subcommands()
            .map(|s| s.get_name().to_string())
            .collect();
        assert!(Shell::value_variants().len() >= 5);
        for shell in Shell::value_variants() {
            assert!(matches!(
                parse(&["completions", &shell.to_string()]).command,
                Commands::Completions { .. }
            ));
            let mut out = Vec::new();
            clap_complete::generate(*shell, &mut Cli::command(), "foundry", &mut out);
            let script = String::from_utf8(out).expect("completion script is UTF-8");
            for name in &subcommands {
                assert!(
                    script.contains(name.as_str()),
                    "{shell} completions are missing `{name}`"
                );
            }
        }
    }
}
