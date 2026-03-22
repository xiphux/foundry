use clap::{Parser, Subcommand};
use clap_complete::Shell;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "foundry", about = "AI agent workspace manager")]
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
    Start {
        /// Name for the feature branch / worktree
        name: String,
    },
    /// Reopen workspace for an existing worktree
    Open {
        /// Worktree name (lists active worktrees if omitted)
        name: Option<String>,
    },
    /// Merge, teardown, delete worktree, archive branch
    Finish {
        /// Worktree name (inferred from cwd if omitted)
        name: Option<String>,
    },
    /// Teardown and delete worktree without merging
    Discard {
        /// Worktree name (inferred from cwd if omitted)
        name: Option<String>,
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
    /// List all active workspaces across all projects
    List,
    /// Show status dashboard of all active workspaces
    Status,
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
