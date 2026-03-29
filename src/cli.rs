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
