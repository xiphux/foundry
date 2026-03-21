# Foundry CLI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Rust CLI that automates AI agent workspace lifecycle using git worktrees and Ghostty terminal automation.

**Architecture:** Single Rust crate with modular internals. Shells out to `git` CLI for all git operations and `osascript` for Ghostty terminal automation. Config is TOML-based with global defaults and per-project overrides.

**Tech Stack:** Rust, clap (CLI), serde + toml (config), anyhow (errors), dirs (paths), which (executable discovery)

**Spec:** `docs/superpowers/specs/2026-03-21-foundry-cli-design.md`

---

## File Map

| File | Responsibility |
|---|---|
| `Cargo.toml` | Crate metadata and dependencies |
| `src/main.rs` | Entry point, CLI dispatch |
| `src/cli.rs` | clap command/arg definitions, shell completions |
| `src/config/mod.rs` | Config loading, merging global + project, template variable resolution |
| `src/config/global.rs` | `GlobalConfig` struct and defaults |
| `src/config/project.rs` | `ProjectConfig` struct |
| `src/config/types.rs` | Shared config types: `PaneConfig`, `ScriptConfig`, `SplitDirection`, `MergeStrategy` |
| `src/git.rs` | Git CLI wrapper functions |
| `src/terminal/mod.rs` | `TerminalBackend` trait, detection dispatch (uses `SplitDirection` from config/types) |
| `src/terminal/ghostty.rs` | Ghostty AppleScript implementation |
| `src/registry.rs` | Project registry (CRUD on `~/.foundry/projects.toml`) |
| `src/state.rs` | Workspace state (CRUD on `~/.foundry/state.toml`) |
| `src/workflow/mod.rs` | Shared workflow helpers (project resolution, name inference) |
| `src/workflow/start.rs` | `start` command orchestration |
| `src/workflow/open.rs` | `open` command orchestration |
| `src/workflow/finish.rs` | `finish` command orchestration |
| `src/workflow/discard.rs` | `discard` command orchestration |
| `tests/` | Integration tests |

---

## Task 1: Project Scaffolding

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`

- [ ] **Step 1: Initialize the Rust project**

Run: `cargo init --name foundry`

This creates `Cargo.toml` and `src/main.rs` with a hello world.

- [ ] **Step 2: Add dependencies to Cargo.toml**

```toml
[package]
name = "foundry"
version = "0.1.0"
edition = "2021"

[dependencies]
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
toml = "0.8"
anyhow = "1"
dirs = "6"
which = "7"
chrono = { version = "0.4", features = ["serde"] }
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: Compiles successfully.

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock src/main.rs
git commit -m "feat: scaffold Rust project with dependencies"
```

---

## Task 2: CLI Definitions

**Files:**
- Create: `src/cli.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write a test for CLI parsing**

Create `tests/cli_test.rs`:

```rust
use std::process::Command;

#[test]
fn test_cli_no_args_shows_help() {
    let output = Command::new(env!("CARGO_BIN_EXE_foundry"))
        .arg("--help")
        .output()
        .expect("failed to run foundry");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("start"));
    assert!(stdout.contains("open"));
    assert!(stdout.contains("finish"));
    assert!(stdout.contains("discard"));
    assert!(stdout.contains("projects"));
    assert!(stdout.contains("list"));
}

#[test]
fn test_cli_start_requires_name() {
    let output = Command::new(env!("CARGO_BIN_EXE_foundry"))
        .arg("start")
        .output()
        .expect("failed to run foundry");
    assert!(!output.status.success());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test cli_test`
Expected: FAIL (binary doesn't have these subcommands yet).

- [ ] **Step 3: Implement the CLI definitions**

Create `src/cli.rs`:

```rust
use clap::{Parser, Subcommand};
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
    /// Manage project registry
    #[command(subcommand)]
    Projects(ProjectsCommands),
    /// List all active workspaces across all projects
    List,
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
```

Update `src/main.rs`:

```rust
mod cli;

use clap::Parser;
use cli::Cli;

fn main() {
    let cli = Cli::parse();

    match cli.command {
        cli::Commands::Start { name } => {
            println!("Starting workspace: {name}");
        }
        cli::Commands::Open { name } => {
            println!("Opening workspace: {}", name.unwrap_or_else(|| "(list)".into()));
        }
        cli::Commands::Finish { name } => {
            println!("Finishing workspace: {}", name.unwrap_or_else(|| "(infer)".into()));
        }
        cli::Commands::Discard { name } => {
            println!("Discarding workspace: {}", name.unwrap_or_else(|| "(infer)".into()));
        }
        cli::Commands::Projects(cmd) => match cmd {
            cli::ProjectsCommands::List => println!("Listing projects"),
            cli::ProjectsCommands::Add { name, path } => {
                println!("Adding project {name} at {}", path.display());
            }
            cli::ProjectsCommands::Remove { name } => {
                println!("Removing project {name}");
            }
        },
        cli::Commands::List => println!("Listing all workspaces"),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test cli_test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/cli.rs src/main.rs tests/cli_test.rs
git commit -m "feat: add CLI definitions with clap"
```

---

## Task 3: Config Types and Parsing

**Files:**
- Create: `src/config/mod.rs`
- Create: `src/config/global.rs`
- Create: `src/config/project.rs`
- Create: `src/config/types.rs`

- [ ] **Step 1: Write tests for config deserialization**

Create `tests/config_test.rs`:

```rust
use std::path::PathBuf;

// We'll test the config types directly
// First, let's make them public for testing

#[test]
fn test_global_config_deserialization() {
    let toml_str = r#"
branch_prefix = "xiphux"
agent_command = "claude"
archive_prefix = "archive"
merge_strategy = "ff-only"
worktree_dir = "~/.foundry/worktrees"

[[panes]]
name = "agent"
command = "{agent_command}"

[[panes]]
name = "git"
command = "lazygit"
split_from = "agent"
direction = "right"

[[panes]]
name = "shell"
split_from = "git"
direction = "down"

[[panes]]
name = "server"
split_from = "shell"
direction = "right"
optional = true
"#;
    let config: foundry::config::GlobalConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.branch_prefix.as_deref(), Some("xiphux"));
    assert_eq!(config.agent_command, "claude");
    assert_eq!(config.merge_strategy, foundry::config::MergeStrategy::FfOnly);
    assert_eq!(config.panes.len(), 4);
    assert!(config.panes[3].optional);
}

#[test]
fn test_global_config_defaults() {
    let toml_str = "";
    let config: foundry::config::GlobalConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.branch_prefix, None);
    assert_eq!(config.agent_command, "claude");
    assert_eq!(config.archive_prefix, "archive");
    assert_eq!(config.merge_strategy, foundry::config::MergeStrategy::FfOnly);
}

#[test]
fn test_project_config_deserialization() {
    let toml_str = r#"
[[scripts.setup]]
name = "Copy env"
command = "cp {source}/.env {worktree}/.env"

[[scripts.teardown]]
name = "Docker down"
command = "docker compose down"
working_dir = "{worktree}"

[panes.server]
command = "npm run serve"
"#;
    let config: foundry::config::ProjectConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.scripts.setup.len(), 1);
    assert_eq!(config.scripts.teardown.len(), 1);
    assert!(config.panes.contains_key("server"));
}

#[test]
fn test_merge_strategy_deserialization() {
    let toml_str = r#"merge_strategy = "merge""#;
    let config: foundry::config::GlobalConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.merge_strategy, foundry::config::MergeStrategy::Merge);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test config_test`
Expected: FAIL (module doesn't exist yet).

- [ ] **Step 3: Implement config types**

Create `src/config/types.rs`:

```rust
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum MergeStrategy {
    FfOnly,
    Merge,
}

impl Default for MergeStrategy {
    fn default() -> Self {
        Self::FfOnly
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SplitDirection {
    Right,
    Down,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PaneConfig {
    pub name: String,
    pub command: Option<String>,
    pub split_from: Option<String>,
    pub direction: Option<SplitDirection>,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScriptConfig {
    pub name: String,
    pub command: String,
    pub working_dir: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ScriptsConfig {
    #[serde(default)]
    pub setup: Vec<ScriptConfig>,
    #[serde(default)]
    pub teardown: Vec<ScriptConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PaneOverride {
    pub command: Option<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
}
```

Create `src/config/global.rs`:

```rust
use serde::Deserialize;

use super::types::{MergeStrategy, PaneConfig};

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct GlobalConfig {
    pub branch_prefix: Option<String>,
    pub agent_command: String,
    pub archive_prefix: String,
    pub merge_strategy: MergeStrategy,
    pub worktree_dir: String,
    #[serde(default)]
    pub panes: Vec<PaneConfig>,
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            branch_prefix: None,
            agent_command: "claude".into(),
            archive_prefix: "archive".into(),
            merge_strategy: MergeStrategy::default(),
            worktree_dir: "~/.foundry/worktrees".into(),
            panes: Vec::new(),
        }
    }
}
```

Create `src/config/project.rs`:

```rust
use serde::Deserialize;
use std::collections::HashMap;

use super::types::{PaneOverride, ScriptsConfig};

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct ProjectConfig {
    pub branch_prefix: Option<String>,
    pub agent_command: Option<String>,
    pub archive_prefix: Option<String>,
    pub merge_strategy: Option<super::types::MergeStrategy>,
    pub worktree_dir: Option<String>,
    #[serde(default)]
    pub scripts: ScriptsConfig,
    #[serde(default)]
    pub panes: HashMap<String, PaneOverride>,
}
```

Create `src/config/mod.rs`:

```rust
mod global;
mod project;
pub mod types;

pub use global::GlobalConfig;
pub use project::ProjectConfig;
pub use types::{MergeStrategy, PaneConfig, PaneOverride, ScriptConfig, SplitDirection};

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Resolved configuration after merging global + project configs.
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub branch_prefix: Option<String>,
    pub agent_command: String,
    pub archive_prefix: String,
    pub merge_strategy: MergeStrategy,
    pub worktree_dir: PathBuf,
    pub panes: Vec<PaneConfig>,
    pub setup_scripts: Vec<ScriptConfig>,
    pub teardown_scripts: Vec<ScriptConfig>,
}

/// Load the global config from ~/.foundry/config.toml.
/// Returns defaults if the file doesn't exist.
pub fn load_global_config() -> Result<GlobalConfig> {
    let config_dir = foundry_dir()?;
    let config_path = config_dir.join("config.toml");

    if !config_path.exists() {
        return Ok(GlobalConfig::default());
    }

    let contents = std::fs::read_to_string(&config_path)
        .with_context(|| format!("failed to read {}", config_path.display()))?;
    let config: GlobalConfig = toml::from_str(&contents)
        .with_context(|| format!("failed to parse {}", config_path.display()))?;

    // Validate template variables in pane commands at parse time
    for pane in &config.panes {
        if let Some(ref cmd) = pane.command {
            validate_template(cmd)
                .with_context(|| format!("in pane '{}' command", pane.name))?;
        }
    }

    Ok(config)
}

/// Load the project config from .foundry.toml in the given repo root.
/// Returns None if the file doesn't exist.
pub fn load_project_config(repo_root: &Path) -> Result<Option<ProjectConfig>> {
    let config_path = repo_root.join(".foundry.toml");

    if !config_path.exists() {
        return Ok(None);
    }

    let contents = std::fs::read_to_string(&config_path)
        .with_context(|| format!("failed to read {}", config_path.display()))?;
    let config: ProjectConfig = toml::from_str(&contents)
        .with_context(|| format!("failed to parse {}", config_path.display()))?;

    // Validate template variables in scripts at parse time
    for script in &config.scripts.setup {
        validate_template(&script.command)
            .with_context(|| format!("in setup script '{}'", script.name))?;
        if let Some(ref wd) = script.working_dir {
            validate_template(wd)
                .with_context(|| format!("in setup script '{}' working_dir", script.name))?;
        }
    }
    for script in &config.scripts.teardown {
        validate_template(&script.command)
            .with_context(|| format!("in teardown script '{}'", script.name))?;
        if let Some(ref wd) = script.working_dir {
            validate_template(wd)
                .with_context(|| format!("in teardown script '{}' working_dir", script.name))?;
        }
    }

    Ok(Some(config))
}

/// Merge global and project configs into a resolved config.
pub fn merge_configs(global: &GlobalConfig, project: Option<&ProjectConfig>) -> ResolvedConfig {
    let worktree_dir_str = project
        .and_then(|p| p.worktree_dir.as_deref())
        .unwrap_or(&global.worktree_dir);

    let worktree_dir = expand_tilde(worktree_dir_str);

    // Merge pane overrides: apply project overrides to global panes,
    // skip optional panes that don't have a project override.
    let panes = global
        .panes
        .iter()
        .filter_map(|pane| {
            let project_override = project.and_then(|p| p.panes.get(&pane.name));

            if pane.optional && project_override.is_none() {
                return None; // Skip optional panes without project opt-in
            }

            let mut merged = pane.clone();
            if let Some(ov) = project_override {
                if let Some(ref cmd) = ov.command {
                    merged.command = Some(cmd.clone());
                }
                if !ov.env.is_empty() {
                    merged.env.extend(ov.env.clone());
                }
            }
            Some(merged)
        })
        .collect();

    ResolvedConfig {
        branch_prefix: project
            .and_then(|p| p.branch_prefix.clone())
            .or_else(|| global.branch_prefix.clone()),
        agent_command: project
            .and_then(|p| p.agent_command.clone())
            .unwrap_or_else(|| global.agent_command.clone()),
        archive_prefix: project
            .and_then(|p| p.archive_prefix.clone())
            .unwrap_or_else(|| global.archive_prefix.clone()),
        merge_strategy: project
            .and_then(|p| p.merge_strategy.clone())
            .unwrap_or_else(|| global.merge_strategy.clone()),
        worktree_dir,
        panes,
        setup_scripts: project
            .map(|p| p.scripts.setup.clone())
            .unwrap_or_default(),
        teardown_scripts: project
            .map(|p| p.scripts.teardown.clone())
            .unwrap_or_default(),
    }
}

/// Expand ~ to home directory.
pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

/// Get the foundry base directory (~/.foundry/).
pub fn foundry_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("could not determine home directory")?;
    Ok(home.join(".foundry"))
}
```

Update `src/main.rs` to add the config module and make it a lib+bin:

Add `src/lib.rs`:

```rust
pub mod config;
```

Update `src/main.rs`:

```rust
mod cli;

use clap::Parser;
use cli::Cli;

fn main() {
    let cli = Cli::parse();

    match cli.command {
        cli::Commands::Start { name } => {
            println!("Starting workspace: {name}");
        }
        cli::Commands::Open { name } => {
            println!("Opening workspace: {}", name.unwrap_or_else(|| "(list)".into()));
        }
        cli::Commands::Finish { name } => {
            println!("Finishing workspace: {}", name.unwrap_or_else(|| "(infer)".into()));
        }
        cli::Commands::Discard { name } => {
            println!("Discarding workspace: {}", name.unwrap_or_else(|| "(infer)".into()));
        }
        cli::Commands::Projects(cmd) => match cmd {
            cli::ProjectsCommands::List => println!("Listing projects"),
            cli::ProjectsCommands::Add { name, path } => {
                println!("Adding project {name} at {}", path.display());
            }
            cli::ProjectsCommands::Remove { name } => {
                println!("Removing project {name}");
            }
        },
        cli::Commands::List => println!("Listing all workspaces"),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test config_test`
Expected: PASS.

- [ ] **Step 5: Write a test for config merging**

Add to `tests/config_test.rs`:

```rust
#[test]
fn test_config_merge_optional_pane_skipped() {
    let global = foundry::config::GlobalConfig {
        panes: vec![
            foundry::config::PaneConfig {
                name: "agent".into(),
                command: Some("claude".into()),
                split_from: None,
                direction: None,
                optional: false,
                env: Default::default(),
            },
            foundry::config::PaneConfig {
                name: "server".into(),
                command: Some("npm run dev".into()),
                split_from: Some("agent".into()),
                direction: Some(foundry::config::SplitDirection::Right),
                optional: true,
                env: Default::default(),
            },
        ],
        ..Default::default()
    };
    let resolved = foundry::config::merge_configs(&global, None);
    assert_eq!(resolved.panes.len(), 1); // server should be skipped
    assert_eq!(resolved.panes[0].name, "agent");
}

#[test]
fn test_config_merge_optional_pane_opted_in() {
    let global = foundry::config::GlobalConfig {
        panes: vec![
            foundry::config::PaneConfig {
                name: "agent".into(),
                command: Some("claude".into()),
                split_from: None,
                direction: None,
                optional: false,
                env: Default::default(),
            },
            foundry::config::PaneConfig {
                name: "server".into(),
                command: Some("npm run dev".into()),
                split_from: Some("agent".into()),
                direction: Some(foundry::config::SplitDirection::Right),
                optional: true,
                env: Default::default(),
            },
        ],
        ..Default::default()
    };
    let project = foundry::config::ProjectConfig {
        panes: std::collections::HashMap::from([(
            "server".into(),
            foundry::config::PaneOverride {
                command: Some("npm run serve".into()),
                env: Default::default(),
            },
        )]),
        ..Default::default()
    };
    let resolved = foundry::config::merge_configs(&global, Some(&project));
    assert_eq!(resolved.panes.len(), 2);
    assert_eq!(resolved.panes[1].command.as_deref(), Some("npm run serve"));
}
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test --test config_test`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/config/ src/lib.rs src/main.rs tests/config_test.rs
git commit -m "feat: add config types, parsing, and merging"
```

---

## Task 4: Template Variable Resolution

**Files:**
- Modify: `src/config/mod.rs`

- [ ] **Step 1: Write tests for template resolution**

Create `tests/template_test.rs`:

```rust
#[test]
fn test_resolve_template_variables() {
    let vars = foundry::config::TemplateVars {
        source: "/Users/me/code/myapp".into(),
        worktree: "/Users/me/.foundry/worktrees/myapp/feat".into(),
        branch: "xiphux/feat".into(),
        name: "feat".into(),
        project: "myapp".into(),
        agent_command: "claude".into(),
    };
    let result = foundry::config::resolve_template("cp {source}/.env {worktree}/.env", &vars).unwrap();
    assert_eq!(result, "cp /Users/me/code/myapp/.env /Users/me/.foundry/worktrees/myapp/feat/.env");
}

#[test]
fn test_resolve_unknown_variable_errors() {
    let vars = foundry::config::TemplateVars {
        source: "".into(),
        worktree: "".into(),
        branch: "".into(),
        name: "".into(),
        project: "".into(),
        agent_command: "".into(),
    };
    let result = foundry::config::resolve_template("echo {unknown}", &vars);
    assert!(result.is_err());
}

#[test]
fn test_resolve_no_variables() {
    let vars = foundry::config::TemplateVars {
        source: "".into(),
        worktree: "".into(),
        branch: "".into(),
        name: "".into(),
        project: "".into(),
        agent_command: "".into(),
    };
    let result = foundry::config::resolve_template("echo hello", &vars).unwrap();
    assert_eq!(result, "echo hello");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test template_test`
Expected: FAIL.

- [ ] **Step 3: Implement template resolution**

Add to `src/config/mod.rs`:

```rust
/// Values available for template variable substitution.
#[derive(Debug, Clone)]
pub struct TemplateVars {
    pub source: String,
    pub worktree: String,
    pub branch: String,
    pub name: String,
    pub project: String,
    pub agent_command: String,
}

/// The set of known template variable names.
const KNOWN_VARS: &[&str] = &["source", "worktree", "branch", "name", "project", "agent_command"];

/// Validate that a template string only uses known variable names.
/// Called at config parse time. Does NOT resolve values.
pub fn validate_template(template: &str) -> Result<()> {
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' {
            let var_name: String = chars.by_ref().take_while(|&c| c != '}').collect();
            if !KNOWN_VARS.contains(&var_name.as_str()) {
                anyhow::bail!("unknown template variable: {{{var_name}}}");
            }
        }
    }
    Ok(())
}

/// Resolve template variables in a string. Returns an error for unknown variables.
pub fn resolve_template(template: &str, vars: &TemplateVars) -> Result<String> {
    let mut result = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '{' {
            let var_name: String = chars.by_ref().take_while(|&c| c != '}').collect();
            let value = match var_name.as_str() {
                "source" => &vars.source,
                "worktree" => &vars.worktree,
                "branch" => &vars.branch,
                "name" => &vars.name,
                "project" => &vars.project,
                "agent_command" => &vars.agent_command,
                _ => anyhow::bail!("unknown template variable: {{{var_name}}}"),
            };
            result.push_str(value);
        } else {
            result.push(c);
        }
    }

    Ok(result)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test template_test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/config/mod.rs tests/template_test.rs
git commit -m "feat: add template variable resolution"
```

---

## Task 5: Git Operations Module

**Files:**
- Create: `src/git.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write tests for git helpers**

Create `tests/git_test.rs`. These are integration tests that create temporary git repos:

```rust
use std::process::Command;
use tempfile::TempDir;

fn init_test_repo() -> TempDir {
    let dir = TempDir::new().unwrap();
    Command::new("git")
        .args(["init"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    Command::new("git")
        .args(["commit", "--allow-empty", "-m", "initial"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    // Rename branch to "main"
    Command::new("git")
        .args(["branch", "-M", "main"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    dir
}

#[test]
fn test_detect_main_branch() {
    let repo = init_test_repo();
    let branch = foundry::git::detect_main_branch(repo.path()).unwrap();
    assert_eq!(branch, "main");
}

#[test]
fn test_detect_master_branch() {
    let repo = init_test_repo();
    Command::new("git")
        .args(["branch", "-M", "master"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    let branch = foundry::git::detect_main_branch(repo.path()).unwrap();
    assert_eq!(branch, "master");
}

#[test]
fn test_create_branch() {
    let repo = init_test_repo();
    foundry::git::create_branch(repo.path(), "feat/test").unwrap();
    let output = Command::new("git")
        .args(["branch", "--list", "feat/test"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("feat/test"));
}

#[test]
fn test_has_uncommitted_changes_clean() {
    let repo = init_test_repo();
    assert!(!foundry::git::has_uncommitted_changes(repo.path()).unwrap());
}

#[test]
fn test_has_uncommitted_changes_dirty() {
    let repo = init_test_repo();
    std::fs::write(repo.path().join("file.txt"), "hello").unwrap();
    assert!(foundry::git::has_uncommitted_changes(repo.path()).unwrap());
}

#[test]
fn test_archive_branch_collision() {
    let repo = init_test_repo();

    // Create and archive a branch
    foundry::git::create_branch(repo.path(), "feat").unwrap();
    foundry::git::archive_branch(repo.path(), "feat", "archive").unwrap();

    // Create the same branch name again and archive it
    foundry::git::create_branch(repo.path(), "feat").unwrap();
    foundry::git::archive_branch(repo.path(), "feat", "archive").unwrap();

    // Both should exist with different timestamps
    let output = Command::new("git")
        .args(["branch", "--list", "archive/feat-*"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    let branches = String::from_utf8_lossy(&output.stdout);
    let count = branches.lines().filter(|l| !l.trim().is_empty()).count();
    assert!(count >= 2, "expected at least 2 archived branches, got {count}: {branches}");
}
```

- [ ] **Step 2: Add `tempfile` dev dependency**

Add to `Cargo.toml`:

```toml
[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --test git_test`
Expected: FAIL (module doesn't exist).

- [ ] **Step 4: Implement git module**

Create `src/git.rs`:

```rust
use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

/// Run a git command and return stdout. Errors if the command fails.
fn run_git(repo_path: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(args)
        .output()
        .context("failed to execute git")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git {} failed: {}", args.join(" "), stderr.trim());
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Detect whether the repo uses "main" or "master" as the primary branch.
pub fn detect_main_branch(repo_path: &Path) -> Result<String> {
    // Try symbolic-ref first (works when origin is set)
    if let Ok(output) = run_git(repo_path, &["symbolic-ref", "refs/remotes/origin/HEAD"]) {
        if let Some(branch) = output.strip_prefix("refs/remotes/origin/") {
            return Ok(branch.to_string());
        }
    }

    // Fall back to checking local branches
    let branches = run_git(repo_path, &["branch", "--list", "--format=%(refname:short)"])?;
    for candidate in ["main", "master"] {
        if branches.lines().any(|b| b == candidate) {
            return Ok(candidate.to_string());
        }
    }

    bail!("could not detect main branch: neither 'main' nor 'master' found")
}

/// Create a new branch at the current HEAD.
pub fn create_branch(repo_path: &Path, name: &str) -> Result<()> {
    run_git(repo_path, &["branch", name])?;
    Ok(())
}

/// Create a worktree at the given path for the given branch.
pub fn create_worktree(repo_path: &Path, worktree_path: &Path, branch: &str) -> Result<()> {
    let path_str = worktree_path.to_str().context("invalid worktree path")?;
    run_git(repo_path, &["worktree", "add", path_str, branch])?;
    Ok(())
}

/// Remove a worktree.
pub fn remove_worktree(repo_path: &Path, worktree_path: &Path, force: bool) -> Result<()> {
    let path_str = worktree_path.to_str().context("invalid worktree path")?;
    let mut args = vec!["worktree", "remove", path_str];
    if force {
        args.push("--force");
    }
    run_git(repo_path, &args)?;
    Ok(())
}

/// Merge a branch using fast-forward only.
pub fn merge_ff_only(repo_path: &Path, branch: &str) -> Result<()> {
    run_git(repo_path, &["merge", "--ff-only", branch])?;
    Ok(())
}

/// Merge a branch (allowing merge commits).
pub fn merge(repo_path: &Path, branch: &str) -> Result<()> {
    let result = run_git(repo_path, &["merge", branch]);
    if result.is_err() {
        // Abort the failed merge to leave the repo clean
        let _ = run_git(repo_path, &["merge", "--abort"]);
        return result;
    }
    Ok(())
}

/// Rename a branch to archive/<branch>-<datestamp>.
/// Appends a date (YYYYMMDD) to avoid collisions when the same branch name
/// is reused. Falls back to datetime (YYYYMMDD-HHMMSS) if the date-only
/// name already exists.
pub fn archive_branch(repo_path: &Path, branch: &str, prefix: &str) -> Result<()> {
    let date = chrono::Utc::now().format("%Y%m%d").to_string();
    let archived = format!("{prefix}/{branch}-{date}");

    // Check if this archive name already exists
    let exists = run_git(repo_path, &["branch", "--list", &archived])
        .map(|out| !out.is_empty())
        .unwrap_or(false);

    let final_name = if exists {
        let datetime = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
        format!("{prefix}/{branch}-{datetime}")
    } else {
        archived
    };

    run_git(repo_path, &["branch", "-m", branch, &final_name])?;
    Ok(())
}

/// Check if the working tree at the given path has uncommitted changes.
pub fn has_uncommitted_changes(repo_path: &Path) -> Result<bool> {
    let output = run_git(repo_path, &["status", "--porcelain"])?;
    Ok(!output.is_empty())
}

/// Get the current branch name.
pub fn current_branch(repo_path: &Path) -> Result<String> {
    run_git(repo_path, &["rev-parse", "--abbrev-ref", "HEAD"])
}

/// Get the root directory of the git repo containing the given path.
pub fn repo_root(path: &Path) -> Result<std::path::PathBuf> {
    let root = run_git(path, &["rev-parse", "--show-toplevel"])?;
    Ok(std::path::PathBuf::from(root))
}
```

Update `src/lib.rs`:

```rust
pub mod config;
pub mod git;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --test git_test`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/git.rs src/lib.rs Cargo.toml tests/git_test.rs
git commit -m "feat: add git CLI wrapper module"
```

---

## Task 6: Project Registry

**Files:**
- Create: `src/registry.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write tests for registry**

Create `tests/registry_test.rs`:

```rust
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn test_registry_add_and_list() {
    let dir = TempDir::new().unwrap();
    let registry_path = dir.path().join("projects.toml");

    let mut registry = foundry::registry::Registry::load_from(&registry_path).unwrap();
    registry.add("myapp", PathBuf::from("/code/myapp")).unwrap();
    registry.save_to(&registry_path).unwrap();

    let reloaded = foundry::registry::Registry::load_from(&registry_path).unwrap();
    assert_eq!(reloaded.get("myapp").unwrap(), PathBuf::from("/code/myapp"));
    assert_eq!(reloaded.list().len(), 1);
}

#[test]
fn test_registry_remove() {
    let dir = TempDir::new().unwrap();
    let registry_path = dir.path().join("projects.toml");

    let mut registry = foundry::registry::Registry::load_from(&registry_path).unwrap();
    registry.add("myapp", PathBuf::from("/code/myapp")).unwrap();
    registry.remove("myapp").unwrap();
    registry.save_to(&registry_path).unwrap();

    let reloaded = foundry::registry::Registry::load_from(&registry_path).unwrap();
    assert!(reloaded.get("myapp").is_none());
}

#[test]
fn test_registry_duplicate_name_errors() {
    let dir = TempDir::new().unwrap();
    let registry_path = dir.path().join("projects.toml");

    let mut registry = foundry::registry::Registry::load_from(&registry_path).unwrap();
    registry.add("myapp", PathBuf::from("/code/myapp")).unwrap();
    let result = registry.add("myapp", PathBuf::from("/code/other"));
    assert!(result.is_err());
}

#[test]
fn test_registry_load_nonexistent_returns_empty() {
    let dir = TempDir::new().unwrap();
    let registry_path = dir.path().join("nonexistent.toml");

    let registry = foundry::registry::Registry::load_from(&registry_path).unwrap();
    assert!(registry.list().is_empty());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test registry_test`
Expected: FAIL.

- [ ] **Step 3: Implement the registry**

Create `src/registry.rs`:

```rust
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct RegistryFile {
    #[serde(default)]
    projects: BTreeMap<String, ProjectEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProjectEntry {
    path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct Registry {
    inner: RegistryFile,
}

impl Registry {
    /// Load the registry from a file. Returns empty registry if file doesn't exist.
    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self {
                inner: RegistryFile::default(),
            });
        }
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let inner: RegistryFile = toml::from_str(&contents)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        Ok(Self { inner })
    }

    /// Save the registry to a file.
    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let contents = toml::to_string_pretty(&self.inner).context("failed to serialize registry")?;
        std::fs::write(path, contents)
            .with_context(|| format!("failed to write {}", path.display()))?;
        Ok(())
    }

    /// Add a project. Errors if the name already exists.
    pub fn add(&mut self, name: &str, path: PathBuf) -> Result<()> {
        if self.inner.projects.contains_key(name) {
            bail!("project '{name}' already exists. Use `foundry projects remove` first, or choose a different name.");
        }
        self.inner.projects.insert(name.to_string(), ProjectEntry { path });
        Ok(())
    }

    /// Remove a project by name. Errors if not found.
    pub fn remove(&mut self, name: &str) -> Result<()> {
        if self.inner.projects.remove(name).is_none() {
            bail!("project '{name}' not found");
        }
        Ok(())
    }

    /// Get the path for a project.
    pub fn get(&self, name: &str) -> Option<PathBuf> {
        self.inner.projects.get(name).map(|e| e.path.clone())
    }

    /// List all registered projects as (name, path) pairs.
    pub fn list(&self) -> Vec<(String, PathBuf)> {
        self.inner
            .projects
            .iter()
            .map(|(k, v)| (k.clone(), v.path.clone()))
            .collect()
    }

    /// Find a project by its path.
    pub fn find_by_path(&self, path: &Path) -> Option<String> {
        self.inner
            .projects
            .iter()
            .find(|(_, v)| v.path == path)
            .map(|(k, _)| k.clone())
    }
}
```

Update `src/lib.rs`:

```rust
pub mod config;
pub mod git;
pub mod registry;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test registry_test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/registry.rs src/lib.rs tests/registry_test.rs
git commit -m "feat: add project registry with CRUD operations"
```

---

## Task 7: Workspace State

**Files:**
- Create: `src/state.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write tests for workspace state**

Create `tests/state_test.rs`:

```rust
use tempfile::TempDir;

#[test]
fn test_state_add_and_list() {
    let dir = TempDir::new().unwrap();
    let state_path = dir.path().join("state.toml");

    let mut state = foundry::state::WorkspaceState::load_from(&state_path).unwrap();
    state.add(foundry::state::Workspace {
        project: "myapp".into(),
        name: "my-feature".into(),
        branch: "xiphux/my-feature".into(),
        worktree_path: "/tmp/worktrees/myapp/my-feature".into(),
        source_path: "/code/myapp".into(),
        created_at: chrono::Utc::now(),
        terminal_tab_id: String::new(),
    });
    state.save_to(&state_path).unwrap();

    let reloaded = foundry::state::WorkspaceState::load_from(&state_path).unwrap();
    assert_eq!(reloaded.list().len(), 1);
    assert_eq!(reloaded.list()[0].name, "my-feature");
}

#[test]
fn test_state_remove() {
    let dir = TempDir::new().unwrap();
    let state_path = dir.path().join("state.toml");

    let mut state = foundry::state::WorkspaceState::load_from(&state_path).unwrap();
    state.add(foundry::state::Workspace {
        project: "myapp".into(),
        name: "feat-a".into(),
        branch: "feat-a".into(),
        worktree_path: "/tmp/worktrees/myapp/feat-a".into(),
        source_path: "/code/myapp".into(),
        created_at: chrono::Utc::now(),
        terminal_tab_id: String::new(),
    });
    state.remove("myapp", "feat-a");
    state.save_to(&state_path).unwrap();

    let reloaded = foundry::state::WorkspaceState::load_from(&state_path).unwrap();
    assert!(reloaded.list().is_empty());
}

#[test]
fn test_state_find_by_project() {
    let dir = TempDir::new().unwrap();
    let state_path = dir.path().join("state.toml");

    let mut state = foundry::state::WorkspaceState::load_from(&state_path).unwrap();
    state.add(foundry::state::Workspace {
        project: "myapp".into(),
        name: "feat-a".into(),
        branch: "feat-a".into(),
        worktree_path: "/tmp/worktrees/myapp/feat-a".into(),
        source_path: "/code/myapp".into(),
        created_at: chrono::Utc::now(),
        terminal_tab_id: String::new(),
    });
    state.add(foundry::state::Workspace {
        project: "other".into(),
        name: "feat-b".into(),
        branch: "feat-b".into(),
        worktree_path: "/tmp/worktrees/other/feat-b".into(),
        source_path: "/code/other".into(),
        created_at: chrono::Utc::now(),
        terminal_tab_id: String::new(),
    });

    let myapp_workspaces = state.find_by_project("myapp");
    assert_eq!(myapp_workspaces.len(), 1);
    assert_eq!(myapp_workspaces[0].name, "feat-a");
}

#[test]
fn test_state_find_by_worktree_path() {
    let dir = TempDir::new().unwrap();
    let state_path = dir.path().join("state.toml");

    let mut state = foundry::state::WorkspaceState::load_from(&state_path).unwrap();
    state.add(foundry::state::Workspace {
        project: "myapp".into(),
        name: "feat-a".into(),
        branch: "feat-a".into(),
        worktree_path: "/tmp/worktrees/myapp/feat-a".into(),
        source_path: "/code/myapp".into(),
        created_at: chrono::Utc::now(),
        terminal_tab_id: String::new(),
    });

    let found = state.find_by_worktree_path("/tmp/worktrees/myapp/feat-a");
    assert!(found.is_some());
    assert_eq!(found.unwrap().name, "feat-a");

    // Also find when cwd is a subdirectory of the worktree
    let found = state.find_by_worktree_path("/tmp/worktrees/myapp/feat-a/src");
    assert!(found.is_some());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test state_test`
Expected: FAIL.

- [ ] **Step 3: Implement workspace state**

Create `src/state.rs`:

```rust
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub project: String,
    pub name: String,
    pub branch: String,
    pub worktree_path: String,
    pub source_path: String,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub terminal_tab_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct StateFile {
    #[serde(default)]
    workspaces: Vec<Workspace>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceState {
    inner: StateFile,
}

impl WorkspaceState {
    /// Load state from file. Returns empty state if file doesn't exist.
    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self {
                inner: StateFile::default(),
            });
        }
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let inner: StateFile = toml::from_str(&contents)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        Ok(Self { inner })
    }

    /// Save state to file.
    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let contents = toml::to_string_pretty(&self.inner).context("failed to serialize state")?;
        std::fs::write(path, contents)
            .with_context(|| format!("failed to write {}", path.display()))?;
        Ok(())
    }

    /// Add a workspace entry.
    pub fn add(&mut self, workspace: Workspace) {
        self.inner.workspaces.push(workspace);
    }

    /// Remove a workspace entry by project and name.
    pub fn remove(&mut self, project: &str, name: &str) {
        self.inner
            .workspaces
            .retain(|w| !(w.project == project && w.name == name));
    }

    /// List all workspaces.
    pub fn list(&self) -> &[Workspace] {
        &self.inner.workspaces
    }

    /// Find workspaces for a given project.
    pub fn find_by_project(&self, project: &str) -> Vec<&Workspace> {
        self.inner
            .workspaces
            .iter()
            .filter(|w| w.project == project)
            .collect()
    }

    /// Find a workspace whose worktree_path matches or contains the given path.
    pub fn find_by_worktree_path(&self, path: &str) -> Option<&Workspace> {
        self.inner.workspaces.iter().find(|w| {
            path == w.worktree_path || path.starts_with(&format!("{}/", w.worktree_path))
        })
    }

    /// Update the terminal_tab_id for a workspace.
    pub fn set_terminal_tab_id(&mut self, project: &str, name: &str, tab_id: String) {
        if let Some(ws) = self
            .inner
            .workspaces
            .iter_mut()
            .find(|w| w.project == project && w.name == name)
        {
            ws.terminal_tab_id = tab_id;
        }
    }

    /// Prune entries whose worktree directories no longer exist on disk.
    pub fn prune_stale(&mut self) {
        self.inner
            .workspaces
            .retain(|w| Path::new(&w.worktree_path).exists());
    }
}
```

Update `src/lib.rs`:

```rust
pub mod config;
pub mod git;
pub mod registry;
pub mod state;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test state_test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/state.rs src/lib.rs tests/state_test.rs
git commit -m "feat: add workspace state tracking"
```

---

## Task 8: Terminal Automation Trait and Ghostty Backend

**Files:**
- Create: `src/terminal/mod.rs`
- Create: `src/terminal/ghostty.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Write tests for terminal detection**

Create `tests/terminal_test.rs`:

```rust
#[test]
fn test_split_direction_deserialization() {
    let toml_str = r#"direction = "right""#;
    #[derive(serde::Deserialize)]
    struct Wrapper {
        direction: foundry::config::SplitDirection,
    }
    let w: Wrapper = toml::from_str(toml_str).unwrap();
    assert_eq!(w.direction, foundry::config::SplitDirection::Right);
}

#[test]
fn test_ghostty_detection_outside_ghostty() {
    // When TERM_PROGRAM is not "ghostty", detection should return None
    // (This test only works if we're not actually running inside Ghostty)
    if std::env::var("TERM_PROGRAM").ok().as_deref() != Some("ghostty") {
        let result = foundry::terminal::detect_terminal();
        assert!(result.is_err());
    }
}
```

Note: We cannot fully integration-test Ghostty automation in CI (requires a running Ghostty instance). We test what we can (types, detection logic) and rely on manual testing for AppleScript.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test terminal_test`
Expected: FAIL.

- [ ] **Step 3: Implement terminal trait and Ghostty backend**

Create `src/terminal/mod.rs`:

```rust
pub mod ghostty;

use anyhow::{bail, Result};
use std::collections::HashMap;
use std::path::Path;

use crate::config::types::SplitDirection;

/// Detect the current terminal and return a boxed automation backend.
pub fn detect_terminal() -> Result<Box<dyn TerminalBackend>> {
    if let Some(term) = ghostty::GhosttyBackend::detect() {
        return Ok(Box::new(term));
    }

    let term_program = std::env::var("TERM_PROGRAM").unwrap_or_else(|_| "unknown".into());
    bail!(
        "unsupported terminal: '{term_program}'. Supported terminals: Ghostty"
    )
}

/// Object-safe trait for terminal automation backends.
/// All pane/tab handles are represented as opaque Strings.
pub trait TerminalBackend {
    /// Open a new tab with working directory set to `path`.
    /// Returns a handle string for the new tab/pane.
    fn open_tab(&self, path: &Path) -> Result<String>;

    /// Split the pane identified by `target` handle in the given direction.
    /// Returns a handle string for the new pane.
    fn split_pane(&self, target: &str, direction: &SplitDirection) -> Result<String>;

    /// Run a command in the pane identified by `target` handle.
    /// Env vars are set before the command.
    fn run_command(
        &self,
        target: &str,
        command: &str,
        env: &HashMap<String, String>,
    ) -> Result<()>;

    /// Close the tab identified by `tab_id` (persisted from a previous session).
    /// Should be a no-op if the tab no longer exists.
    fn close_tab(&self, tab_id: &str) -> Result<()>;
}
```

Create `src/terminal/ghostty.rs`:

**IMPORTANT:** The AppleScript API for Ghostty must be researched from the Ghostty scripting definition file (`.sdef`) during implementation. The reference is at:
- https://ghostty.org/docs/features/applescript
- https://github.com/ghostty-org/ghostty/blob/main/macos/Ghostty.sdef

The implementation below is a **skeleton** that captures the correct structure. The actual AppleScript commands MUST be verified against the `.sdef` and tested against a running Ghostty instance. Key areas to research:
- How to create a new tab and get a reference to it
- How to create splits targeting a specific pane (not just "current")
- How to send commands to a specific pane by reference
- How to get a stable tab/pane identifier for persistence
- How to close a specific tab by identifier

```rust
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use crate::config::types::SplitDirection;
use super::TerminalBackend;

pub struct GhosttyBackend;

impl GhosttyBackend {
    /// Detect if we're running inside Ghostty.
    pub fn detect() -> Option<Self> {
        let term = std::env::var("TERM_PROGRAM").ok()?;
        if term.eq_ignore_ascii_case("ghostty") {
            Some(Self)
        } else {
            None
        }
    }

    fn run_applescript(script: &str) -> Result<String> {
        let output = Command::new("osascript")
            .arg("-e")
            .arg(script)
            .output()
            .context("failed to run osascript")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("AppleScript error: {}", stderr.trim());
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

impl TerminalBackend for GhosttyBackend {
    fn open_tab(&self, path: &Path) -> Result<String> {
        let path_str = path.to_str().context("invalid path")?;

        // TODO: Research Ghostty .sdef for exact tab creation API.
        // Need to: create a tab, get its ID/reference, cd to path.
        // The script below is a starting point — verify against .sdef.
        let script = format!(
            r#"tell application "Ghostty"
    tell front window
        set newTab to make new tab
        -- TODO: get tab ID for persistence
        -- TODO: write command to set working directory
    end tell
end tell"#
        );
        let result = Self::run_applescript(&script)?;

        // Send cd command to the new tab
        self.run_command(&result, &format!("cd {path_str}"), &HashMap::new())?;

        // Return the tab reference as a handle
        // TODO: Extract actual tab ID from AppleScript result for persistence
        Ok(result)
    }

    fn split_pane(&self, target: &str, direction: &SplitDirection) -> Result<String> {
        let dir_str = match direction {
            SplitDirection::Right => "right",
            SplitDirection::Down => "down",
        };

        // TODO: Research Ghostty .sdef for split API.
        // Need to: target the specific pane (not just "current"), create split,
        // get the new pane's reference.
        let script = format!(
            r#"tell application "Ghostty"
    tell front window
        -- TODO: target specific pane using handle '{target}'
        -- TODO: create split in direction "{dir_str}"
        -- TODO: return new pane reference
    end tell
end tell"#
        );
        let result = Self::run_applescript(&script)?;
        Ok(result)
    }

    fn run_command(
        &self,
        _target: &str,
        command: &str,
        env: &HashMap<String, String>,
    ) -> Result<()> {
        // Build the full command with properly quoted env vars
        let full_command = if env.is_empty() {
            command.to_string()
        } else {
            let env_prefix: Vec<String> = env
                .iter()
                .map(|(k, v)| {
                    let escaped_v = v.replace('\'', "'\\''");
                    format!("export {k}='{escaped_v}'")
                })
                .collect();
            format!("{} && {command}", env_prefix.join(" && "))
        };

        // TODO: Research Ghostty .sdef for how to send text/commands to a pane.
        // Options may include: keystroke injection, or a Ghostty-specific
        // "write" command if available in the scripting definition.
        // Using `target` handle to address the correct pane.
        let escaped = full_command.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!(
            r#"tell application "Ghostty"
    tell front window
        -- TODO: target specific pane, send command text
        -- Placeholder: write to current surface
    end tell
end tell"#
        );
        Self::run_applescript(&script)?;

        Ok(())
    }

    fn close_tab(&self, tab_id: &str) -> Result<()> {
        if tab_id.is_empty() {
            return Ok(()); // No tab to close
        }

        // TODO: Research Ghostty .sdef for how to close a specific tab by ID.
        let script = format!(
            r#"tell application "Ghostty"
    tell front window
        -- TODO: close tab identified by '{tab_id}'
    end tell
end tell"#
        );

        // Ignore errors — tab may already be closed
        let _ = Self::run_applescript(&script);
        Ok(())
    }
}
```

Update `src/lib.rs`:

```rust
pub mod config;
pub mod git;
pub mod registry;
pub mod state;
pub mod terminal;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test terminal_test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/terminal/ src/lib.rs tests/terminal_test.rs
git commit -m "feat: add terminal automation trait and Ghostty backend"
```

---

## Task 9: Workflow — `start` Command

**Files:**
- Create: `src/workflow/mod.rs`
- Create: `src/workflow/start.rs`
- Modify: `src/main.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Implement workflow helpers**

Create `src/workflow/mod.rs`:

```rust
pub mod start;
pub mod open;
pub mod finish;
pub mod discard;

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::config;
use crate::git;
use crate::registry::Registry;
use crate::state::WorkspaceState;

/// Resolve which project we're working with.
/// Uses --project flag if given, otherwise infers from cwd.
pub fn resolve_project(
    project_flag: Option<&str>,
    registry: &mut Registry,
    registry_path: &Path,
) -> Result<(String, PathBuf)> {
    if let Some(name) = project_flag {
        let path = registry
            .get(name)
            .with_context(|| format!("project '{name}' not found. Register it with `foundry projects add`."))?;
        return Ok((name.to_string(), path));
    }

    // Infer from cwd
    let cwd = std::env::current_dir().context("failed to get current directory")?;
    let repo_root = git::repo_root(&cwd).context("not inside a git repository")?;

    // Check if this repo is already registered
    if let Some(name) = registry.find_by_path(&repo_root) {
        return Ok((name, repo_root));
    }

    // Auto-register
    let name = repo_root
        .file_name()
        .context("repo root has no directory name")?
        .to_str()
        .context("directory name is not valid UTF-8")?
        .to_string();

    // Check for collision
    if registry.get(&name).is_some() {
        anyhow::bail!(
            "project name '{name}' is already registered to a different path. \
             Use `foundry projects add <custom-name> {}` to register with a different name.",
            repo_root.display()
        );
    }

    eprintln!("Auto-registering project '{name}' at {}", repo_root.display());
    registry.add(&name, repo_root.clone())?;
    registry.save_to(registry_path)?;

    Ok((name, repo_root))
}

/// Compute the full branch name with optional prefix.
pub fn compute_branch_name(name: &str, prefix: Option<&str>) -> String {
    match prefix {
        Some(p) if !p.is_empty() => format!("{p}/{name}"),
        _ => name.to_string(),
    }
}

/// Get the paths for registry and state files.
pub fn foundry_paths() -> Result<(PathBuf, PathBuf)> {
    let foundry_dir = config::foundry_dir()?;
    Ok((
        foundry_dir.join("projects.toml"),
        foundry_dir.join("state.toml"),
    ))
}
```

- [ ] **Step 2: Implement the start workflow**

Create `src/workflow/start.rs`:

```rust
use anyhow::{Context, Result};
use chrono::Utc;
use std::path::Path;
use std::process::Command;

use crate::config::{self, ResolvedConfig, TemplateVars};
use crate::git;
use crate::state::{Workspace, WorkspaceState};
use crate::terminal;

pub fn run(
    name: &str,
    project_name: &str,
    source_path: &Path,
    config: &ResolvedConfig,
    state: &mut WorkspaceState,
    state_path: &Path,
    verbose: bool,
) -> Result<()> {
    let branch = super::compute_branch_name(name, config.branch_prefix.as_deref());
    let worktree_path = config.worktree_dir.join(project_name).join(name);

    // Check if worktree already exists (idempotent start)
    if worktree_path.exists() {
        if verbose {
            eprintln!("Worktree already exists at {}, opening workspace...", worktree_path.display());
        }
        return super::open::open_workspace(project_name, name, &worktree_path, config, state, state_path, verbose);
    }

    // Create branch
    if verbose {
        eprintln!("Creating branch '{branch}'...");
    }
    git::create_branch(source_path, &branch)
        .with_context(|| format!("failed to create branch '{branch}'"))?;

    // Create worktree directory parent
    if let Some(parent) = worktree_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }

    // Create worktree
    if verbose {
        eprintln!("Creating worktree at {}...", worktree_path.display());
    }
    git::create_worktree(source_path, &worktree_path, &branch)
        .with_context(|| "failed to create worktree")?;

    // Record workspace in state BEFORE setup scripts, so discard can clean up if setup fails
    state.add(Workspace {
        project: project_name.into(),
        name: name.into(),
        branch: branch.clone(),
        worktree_path: worktree_path.to_string_lossy().into(),
        source_path: source_path.to_string_lossy().into(),
        created_at: Utc::now(),
        terminal_tab_id: String::new(),
    });
    state.save_to(state_path)?;

    // Run setup scripts
    let template_vars = TemplateVars {
        source: source_path.to_string_lossy().into(),
        worktree: worktree_path.to_string_lossy().into(),
        branch: branch.clone(),
        name: name.into(),
        project: project_name.into(),
        agent_command: config.agent_command.clone(),
    };

    for script in &config.setup_scripts {
        let resolved_command = config::resolve_template(&script.command, &template_vars)
            .with_context(|| format!("failed to resolve template in script '{}'", script.name))?;

        let working_dir = if let Some(ref wd) = script.working_dir {
            config::resolve_template(wd, &template_vars)?
        } else {
            worktree_path.to_string_lossy().into()
        };

        if verbose {
            eprintln!("Running setup script: {}...", script.name);
        }

        let status = Command::new("sh")
            .arg("-c")
            .arg(&resolved_command)
            .current_dir(&working_dir)
            .status()
            .with_context(|| format!("failed to run setup script '{}'", script.name))?;

        if !status.success() {
            anyhow::bail!(
                "setup script '{}' failed with exit code {}. \
                 Worktree left in place at {}. \
                 Fix the issue and re-run `foundry start {name}`, or clean up with `foundry discard {name}`.",
                script.name,
                status.code().unwrap_or(-1),
                worktree_path.display()
            );
        }
    }

    // Open workspace
    super::open::open_workspace(project_name, name, &worktree_path, config, state, state_path, verbose)
}
```

- [ ] **Step 3: Create stub open module (needed by start)**

Create `src/workflow/open.rs`:

```rust
use anyhow::Result;
use std::collections::HashMap;
use std::path::Path;

use crate::config::{self, ResolvedConfig, TemplateVars};
use crate::state::WorkspaceState;
use crate::terminal::{self, TerminalBackend};

/// Open the terminal workspace for an existing worktree.
pub fn open_workspace(
    project_name: &str,
    name: &str,
    worktree_path: &Path,
    config: &ResolvedConfig,
    state: &mut WorkspaceState,
    state_path: &Path,
    verbose: bool,
) -> Result<()> {
    let backend = terminal::detect_terminal()?;

    // Build template vars for pane commands from workspace state
    let workspace = state
        .find_by_worktree_path(&worktree_path.to_string_lossy());
    let source_path = workspace.map(|w| w.source_path.clone()).unwrap_or_default();
    let branch = workspace.map(|w| w.branch.clone()).unwrap_or_default();

    let template_vars = TemplateVars {
        source: source_path,
        worktree: worktree_path.to_string_lossy().into(),
        branch,
        name: name.into(),
        project: project_name.into(),
        agent_command: config.agent_command.clone(),
    };

    // Open the first pane as a new tab
    let mut pane_handles: HashMap<String, String> = HashMap::new();

    if config.panes.is_empty() {
        // No pane config — just open a tab
        backend.open_tab(worktree_path)?;
        return Ok(());
    }

    // First pane becomes the tab
    let first = &config.panes[0];
    if verbose {
        eprintln!("Opening tab for pane '{}'...", first.name);
    }
    let handle = backend.open_tab(worktree_path)?;

    // Run the first pane's command
    if let Some(ref cmd) = first.command {
        let resolved = config::resolve_template(cmd, &template_vars)?;
        if !resolved.is_empty() {
            backend.run_command(&handle, &resolved, &first.env)?;
        }
    }
    pane_handles.insert(first.name.clone(), handle.clone());

    // Process remaining panes
    for pane in &config.panes[1..] {
        let split_from = pane
            .split_from
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("pane '{}' has no split_from", pane.name))?;

        let parent_handle = pane_handles
            .get(split_from)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "pane '{}' references unknown split_from '{}'",
                    pane.name,
                    split_from
                )
            })?;

        let direction = pane
            .direction
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("pane '{}' has no direction", pane.name))?;

        if verbose {
            eprintln!("Splitting pane '{}'...", pane.name);
        }

        let new_handle = backend.split_pane(parent_handle, direction)?;

        // Run the pane's command
        if let Some(ref cmd) = pane.command {
            let resolved = config::resolve_template(cmd, &template_vars)?;
            if !resolved.is_empty() {
                backend.run_command(&new_handle, &resolved, &pane.env)?;
            }
        }

        pane_handles.insert(pane.name.clone(), new_handle);
    }

    // Persist tab ID in state
    // Use the first pane's handle to get the tab ID
    // (In a real implementation we'd get the actual Ghostty tab ID)
    state.set_terminal_tab_id(project_name, name, handle);
    state.save_to(state_path)?;

    Ok(())
}

/// List active worktrees for a project.
pub fn list_workspaces(state: &WorkspaceState, project: &str) {
    let workspaces = state.find_by_project(project);
    if workspaces.is_empty() {
        println!("No active workspaces for project '{project}'.");
        return;
    }
    println!("Active workspaces for '{project}':");
    for ws in workspaces {
        println!("  {} (branch: {}, path: {})", ws.name, ws.branch, ws.worktree_path);
    }
}
```

Create stub `src/workflow/finish.rs`:

```rust
// Implemented in Task 10
```

Create stub `src/workflow/discard.rs`:

```rust
// Implemented in Task 11
```

- [ ] **Step 4: Wire up main.rs to use the start workflow**

Update `src/main.rs`:

```rust
mod cli;

use anyhow::Result;
use clap::Parser;
use cli::Cli;
use foundry::config;
use foundry::registry::Registry;
use foundry::state::WorkspaceState;
use foundry::workflow;

fn main() -> Result<()> {
    let cli = Cli::parse();

    let (registry_path, state_path) = workflow::foundry_paths()?;

    match cli.command {
        cli::Commands::Start { name } => {
            let mut registry = Registry::load_from(&registry_path)?;
            let (project_name, source_path) =
                workflow::resolve_project(cli.project.as_deref(), &mut registry, &registry_path)?;

            let global_config = config::load_global_config()?;
            let project_config = config::load_project_config(&source_path)?;
            let resolved = config::merge_configs(&global_config, project_config.as_ref());

            let mut state = WorkspaceState::load_from(&state_path)?;

            workflow::start::run(
                &name,
                &project_name,
                &source_path,
                &resolved,
                &mut state,
                &state_path,
                cli.verbose,
            )?;
        }
        cli::Commands::Open { name } => {
            let mut state = WorkspaceState::load_from(&state_path)?;
            state.prune_stale();

            if let Some(name) = name {
                let mut registry = Registry::load_from(&registry_path)?;
                let (project_name, source_path) =
                    workflow::resolve_project(cli.project.as_deref(), &mut registry, &registry_path)?;
                let global_config = config::load_global_config()?;
                let project_config = config::load_project_config(&source_path)?;
                let resolved = config::merge_configs(&global_config, project_config.as_ref());

                let worktree_path = resolved.worktree_dir.join(&project_name).join(&name);
                if !worktree_path.exists() {
                    anyhow::bail!("worktree '{name}' does not exist. Use `foundry start {name}` to create it.");
                }

                workflow::open::open_workspace(
                    &project_name, &name, &worktree_path, &resolved, &mut state, &state_path, cli.verbose,
                )?;
            } else {
                let mut registry = Registry::load_from(&registry_path)?;
                let (project_name, _) =
                    workflow::resolve_project(cli.project.as_deref(), &mut registry, &registry_path)?;
                workflow::open::list_workspaces(&state, &project_name);
            }
        }
        cli::Commands::Finish { name } => {
            println!("Finish: not yet implemented");
        }
        cli::Commands::Discard { name } => {
            println!("Discard: not yet implemented");
        }
        cli::Commands::Projects(cmd) => match cmd {
            cli::ProjectsCommands::List => {
                let registry = Registry::load_from(&registry_path)?;
                let projects = registry.list();
                if projects.is_empty() {
                    println!("No registered projects.");
                } else {
                    for (name, path) in &projects {
                        println!("  {name}: {}", path.display());
                    }
                }
            }
            cli::ProjectsCommands::Add { name, path } => {
                let mut registry = Registry::load_from(&registry_path)?;
                let abs_path = std::fs::canonicalize(&path)
                    .unwrap_or(path);
                registry.add(&name, abs_path)?;
                registry.save_to(&registry_path)?;
                println!("Project '{name}' registered.");
            }
            cli::ProjectsCommands::Remove { name } => {
                let mut registry = Registry::load_from(&registry_path)?;
                let state = WorkspaceState::load_from(&state_path)?;
                let active = state.find_by_project(&name);
                if !active.is_empty() {
                    eprintln!(
                        "Warning: project '{name}' has {} active workspace(s). \
                         Finish or discard them first.",
                        active.len()
                    );
                }
                registry.remove(&name)?;
                registry.save_to(&registry_path)?;
                println!("Project '{name}' removed.");
            }
        },
        cli::Commands::List => {
            let mut state = WorkspaceState::load_from(&state_path)?;
            state.prune_stale();
            state.save_to(&state_path)?;
            let workspaces = state.list();
            if workspaces.is_empty() {
                println!("No active workspaces.");
            } else {
                for ws in workspaces {
                    println!(
                        "  {}/{} (branch: {}, path: {})",
                        ws.project, ws.name, ws.branch, ws.worktree_path
                    );
                }
            }
        }
    }

    Ok(())
}
```

Update `src/lib.rs`:

```rust
pub mod config;
pub mod git;
pub mod registry;
pub mod state;
pub mod terminal;
pub mod workflow;
```

- [ ] **Step 5: Verify it compiles**

Run: `cargo build`
Expected: Compiles successfully.

- [ ] **Step 6: Commit**

```bash
git add src/ tests/
git commit -m "feat: implement start and open workflows with terminal automation"
```

---

## Task 10: Workflow — `finish` Command

**Files:**
- Modify: `src/workflow/finish.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Implement the finish workflow**

Replace `src/workflow/finish.rs`:

```rust
use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

use crate::config::{self, ResolvedConfig, MergeStrategy, TemplateVars};
use crate::git;
use crate::state::WorkspaceState;
use crate::terminal;

pub fn run(
    name: &str,
    project_name: &str,
    source_path: &Path,
    config: &ResolvedConfig,
    state: &mut WorkspaceState,
    state_path: &Path,
    verbose: bool,
) -> Result<()> {
    let worktree_path = config.worktree_dir.join(project_name).join(name);

    if !worktree_path.exists() {
        anyhow::bail!("worktree '{name}' does not exist at {}", worktree_path.display());
    }

    // Look up the workspace in state
    let workspace = state
        .find_by_worktree_path(&worktree_path.to_string_lossy())
        .ok_or_else(|| anyhow::anyhow!("workspace '{name}' not found in state"))?;
    let branch = workspace.branch.clone();
    let tab_id = workspace.terminal_tab_id.clone();

    // Check for uncommitted changes in worktree
    if git::has_uncommitted_changes(&worktree_path)? {
        anyhow::bail!(
            "worktree '{}' has uncommitted changes. Commit or stash them before finishing.",
            worktree_path.display()
        );
    }

    // Check for uncommitted changes in main repo
    if git::has_uncommitted_changes(source_path)? {
        anyhow::bail!(
            "main repo at '{}' has uncommitted changes. Commit or stash them before finishing.",
            source_path.display()
        );
    }

    // Close terminal tab (if open)
    if !tab_id.is_empty() {
        if verbose {
            eprintln!("Closing terminal tab...");
        }
        if let Ok(backend) = terminal::detect_terminal() {
            let _ = backend.close_tab(&tab_id);
        }
    }

    // Run teardown scripts
    let template_vars = TemplateVars {
        source: source_path.to_string_lossy().into(),
        worktree: worktree_path.to_string_lossy().into(),
        branch: branch.clone(),
        name: name.into(),
        project: project_name.into(),
        agent_command: config.agent_command.clone(),
    };

    for script in &config.teardown_scripts {
        let resolved_command = config::resolve_template(&script.command, &template_vars)?;
        let working_dir = if let Some(ref wd) = script.working_dir {
            config::resolve_template(wd, &template_vars)?
        } else {
            worktree_path.to_string_lossy().into()
        };

        if verbose {
            eprintln!("Running teardown script: {}...", script.name);
        }

        let status = Command::new("sh")
            .arg("-c")
            .arg(&resolved_command)
            .current_dir(&working_dir)
            .status()
            .with_context(|| format!("failed to run teardown script '{}'", script.name))?;

        if !status.success() {
            eprintln!(
                "Warning: teardown script '{}' failed (exit code {}), continuing...",
                script.name,
                status.code().unwrap_or(-1)
            );
        }
    }

    // Detect main branch
    let main_branch = git::detect_main_branch(source_path)?;

    // Verify we're on the main branch in the source repo
    let current = git::current_branch(source_path)?;
    if current != main_branch {
        anyhow::bail!(
            "main repo is on branch '{current}', expected '{main_branch}'. \
             Checkout '{main_branch}' before finishing."
        );
    }

    // Merge
    if verbose {
        eprintln!("Merging '{branch}' into '{main_branch}'...");
    }
    match config.merge_strategy {
        MergeStrategy::FfOnly => {
            git::merge_ff_only(source_path, &branch).with_context(|| {
                format!(
                    "fast-forward merge failed. Rebase '{branch}' onto '{main_branch}' first, \
                     then re-run `foundry finish {name}`."
                )
            })?;
        }
        MergeStrategy::Merge => {
            git::merge(source_path, &branch).with_context(|| {
                format!(
                    "merge failed due to conflicts. Resolve conflicts manually, \
                     then re-run `foundry finish {name}`."
                )
            })?;
        }
    }

    // Remove worktree
    if verbose {
        eprintln!("Removing worktree...");
    }
    git::remove_worktree(source_path, &worktree_path, false)?;

    // Archive branch
    if verbose {
        eprintln!("Archiving branch '{branch}'...");
    }
    git::archive_branch(source_path, &branch, &config.archive_prefix)?;

    // Remove from state
    state.remove(project_name, name);
    state.save_to(state_path)?;

    eprintln!("Finished workspace '{name}'. Branch '{branch}' archived.");

    Ok(())
}
```

- [ ] **Step 2: Wire up finish in main.rs**

Update the `Finish` match arm in `src/main.rs`:

```rust
        cli::Commands::Finish { name } => {
            let mut state = WorkspaceState::load_from(&state_path)?;

            let name = match name {
                Some(n) => n,
                None => {
                    // Infer from cwd
                    let cwd = std::env::current_dir()?;
                    let cwd_str = cwd.to_string_lossy();
                    state
                        .find_by_worktree_path(&cwd_str)
                        .map(|w| w.name.clone())
                        .ok_or_else(|| anyhow::anyhow!(
                            "could not infer workspace from current directory. Provide a name: `foundry finish <name>`"
                        ))?
                }
            };

            let mut registry = Registry::load_from(&registry_path)?;
            let (project_name, source_path) =
                workflow::resolve_project(cli.project.as_deref(), &mut registry, &registry_path)?;
            let global_config = config::load_global_config()?;
            let project_config = config::load_project_config(&source_path)?;
            let resolved = config::merge_configs(&global_config, project_config.as_ref());

            workflow::finish::run(
                &name, &project_name, &source_path, &resolved, &mut state, &state_path, cli.verbose,
            )?;
        }
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: Compiles successfully.

- [ ] **Step 4: Commit**

```bash
git add src/workflow/finish.rs src/main.rs
git commit -m "feat: implement finish workflow with merge and cleanup"
```

---

## Task 11: Workflow — `discard` Command

**Files:**
- Modify: `src/workflow/discard.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Implement the discard workflow**

Replace `src/workflow/discard.rs`:

```rust
use anyhow::{Context, Result};
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;

use crate::config::{self, ResolvedConfig, TemplateVars};
use crate::git;
use crate::state::WorkspaceState;
use crate::terminal;

pub fn run(
    name: &str,
    project_name: &str,
    source_path: &Path,
    config: &ResolvedConfig,
    state: &mut WorkspaceState,
    state_path: &Path,
    verbose: bool,
    skip_confirm: bool,
) -> Result<()> {
    let worktree_path = config.worktree_dir.join(project_name).join(name);

    if !worktree_path.exists() {
        anyhow::bail!("worktree '{name}' does not exist at {}", worktree_path.display());
    }

    let workspace = state
        .find_by_worktree_path(&worktree_path.to_string_lossy())
        .ok_or_else(|| anyhow::anyhow!("workspace '{name}' not found in state"))?;
    let branch = workspace.branch.clone();
    let tab_id = workspace.terminal_tab_id.clone();

    // Warn about uncommitted changes
    if git::has_uncommitted_changes(&worktree_path)? && !skip_confirm {
        print!("Worktree has uncommitted changes. Discard anyway? [y/N] ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            eprintln!("Aborted.");
            return Ok(());
        }
    }

    // Close terminal tab
    if !tab_id.is_empty() {
        if verbose {
            eprintln!("Closing terminal tab...");
        }
        if let Ok(backend) = terminal::detect_terminal() {
            let _ = backend.close_tab(&tab_id);
        }
    }

    // Run teardown scripts
    let template_vars = TemplateVars {
        source: source_path.to_string_lossy().into(),
        worktree: worktree_path.to_string_lossy().into(),
        branch: branch.clone(),
        name: name.into(),
        project: project_name.into(),
        agent_command: config.agent_command.clone(),
    };

    for script in &config.teardown_scripts {
        let resolved_command = config::resolve_template(&script.command, &template_vars)?;
        let working_dir = if let Some(ref wd) = script.working_dir {
            config::resolve_template(wd, &template_vars)?
        } else {
            worktree_path.to_string_lossy().into()
        };

        if verbose {
            eprintln!("Running teardown script: {}...", script.name);
        }

        let status = Command::new("sh")
            .arg("-c")
            .arg(&resolved_command)
            .current_dir(&working_dir)
            .status()
            .with_context(|| format!("failed to run teardown script '{}'", script.name))?;

        if !status.success() {
            eprintln!(
                "Warning: teardown script '{}' failed (exit code {}), continuing...",
                script.name,
                status.code().unwrap_or(-1)
            );
        }
    }

    // Remove worktree (force)
    if verbose {
        eprintln!("Removing worktree...");
    }
    git::remove_worktree(source_path, &worktree_path, true)?;

    // Archive branch
    if verbose {
        eprintln!("Archiving branch '{branch}'...");
    }
    git::archive_branch(source_path, &branch, &config.archive_prefix)?;

    // Remove from state
    state.remove(project_name, name);
    state.save_to(state_path)?;

    eprintln!("Discarded workspace '{name}'. Branch '{branch}' archived.");

    Ok(())
}
```

- [ ] **Step 2: Wire up discard in main.rs**

Update the `Discard` match arm in `src/main.rs`:

```rust
        cli::Commands::Discard { name } => {
            let mut state = WorkspaceState::load_from(&state_path)?;

            let name = match name {
                Some(n) => n,
                None => {
                    let cwd = std::env::current_dir()?;
                    let cwd_str = cwd.to_string_lossy();
                    state
                        .find_by_worktree_path(&cwd_str)
                        .map(|w| w.name.clone())
                        .ok_or_else(|| anyhow::anyhow!(
                            "could not infer workspace from current directory. Provide a name: `foundry discard <name>`"
                        ))?
                }
            };

            let mut registry = Registry::load_from(&registry_path)?;
            let (project_name, source_path) =
                workflow::resolve_project(cli.project.as_deref(), &mut registry, &registry_path)?;
            let global_config = config::load_global_config()?;
            let project_config = config::load_project_config(&source_path)?;
            let resolved = config::merge_configs(&global_config, project_config.as_ref());

            workflow::discard::run(
                &name, &project_name, &source_path, &resolved, &mut state, &state_path,
                cli.verbose, cli.yes,
            )?;
        }
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo build`
Expected: Compiles successfully.

- [ ] **Step 4: Commit**

```bash
git add src/workflow/discard.rs src/main.rs
git commit -m "feat: implement discard workflow"
```

---

## Task 12: Shell Completions

**Files:**
- Modify: `src/cli.rs`

- [ ] **Step 1: Add shell completion generation command**

Add to `src/cli.rs` — add a `Completions` variant to `Commands`:

```rust
use clap_complete::Shell;

// Add to Commands enum:
    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        shell: Shell,
    },
```

- [ ] **Step 2: Add clap_complete dependency**

Add to `Cargo.toml`:

```toml
clap_complete = "4"
```

- [ ] **Step 3: Wire up completions in main.rs**

Add to the match in `main.rs`:

```rust
        cli::Commands::Completions { shell } => {
            clap_complete::generate(
                shell,
                &mut Cli::command(),
                "foundry",
                &mut std::io::stdout(),
            );
        }
```

Add `use clap::CommandFactory;` at the top of `main.rs`.

- [ ] **Step 4: Test that completions generate**

Run: `cargo run -- completions zsh | head -5`
Expected: Outputs zsh completion script.

- [ ] **Step 5: Commit**

```bash
git add src/cli.rs src/main.rs Cargo.toml
git commit -m "feat: add shell completion generation"
```

---

## Task 13: Integration Test — Full Start/Finish Cycle

**Files:**
- Create: `tests/integration_test.rs`

- [ ] **Step 1: Write an integration test for the start→finish cycle**

This test creates a temp repo, runs the git operations (without terminal automation since we can't test that in CI), and verifies state.

```rust
use std::process::Command;
use tempfile::TempDir;

fn init_test_repo(dir: &std::path::Path) {
    Command::new("git").args(["init"]).current_dir(dir).output().unwrap();
    Command::new("git").args(["commit", "--allow-empty", "-m", "initial"]).current_dir(dir).output().unwrap();
    Command::new("git").args(["branch", "-M", "main"]).current_dir(dir).output().unwrap();
}

#[test]
fn test_git_workflow_start_to_finish() {
    let repo_dir = TempDir::new().unwrap();
    let worktree_base = TempDir::new().unwrap();
    init_test_repo(repo_dir.path());

    let source = repo_dir.path();
    let worktree_path = worktree_base.path().join("myapp").join("my-feature");

    // Create branch and worktree
    foundry::git::create_branch(source, "xiphux/my-feature").unwrap();
    std::fs::create_dir_all(worktree_path.parent().unwrap()).unwrap();
    foundry::git::create_worktree(source, &worktree_path, "xiphux/my-feature").unwrap();

    // Make a commit in the worktree
    std::fs::write(worktree_path.join("feature.txt"), "hello").unwrap();
    Command::new("git").args(["add", "."]).current_dir(&worktree_path).output().unwrap();
    Command::new("git").args(["commit", "-m", "add feature"]).current_dir(&worktree_path).output().unwrap();

    // Verify no uncommitted changes
    assert!(!foundry::git::has_uncommitted_changes(&worktree_path).unwrap());
    assert!(!foundry::git::has_uncommitted_changes(source).unwrap());

    // Merge ff-only
    foundry::git::merge_ff_only(source, "xiphux/my-feature").unwrap();

    // Remove worktree
    foundry::git::remove_worktree(source, &worktree_path, false).unwrap();

    // Archive branch
    foundry::git::archive_branch(source, "xiphux/my-feature", "archive").unwrap();

    // Verify: feature.txt should be in main now
    let output = Command::new("git")
        .args(["log", "--oneline", "-1"])
        .current_dir(source)
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&output.stdout);
    assert!(log.contains("add feature"));

    // Verify: branch should be archived
    let output = Command::new("git")
        .args(["branch", "--list", "archive/xiphux/my-feature"])
        .current_dir(source)
        .output()
        .unwrap();
    let branches = String::from_utf8_lossy(&output.stdout);
    assert!(branches.contains("archive/xiphux/my-feature"));
}
```

- [ ] **Step 2: Run the integration test**

Run: `cargo test --test integration_test`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add tests/integration_test.rs
git commit -m "test: add integration test for start-to-finish git workflow"
```

---

## Task 14: Final Verification

- [ ] **Step 1: Run all tests**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings.

- [ ] **Step 3: Build release binary**

Run: `cargo build --release`
Expected: Binary at `target/release/foundry`.

- [ ] **Step 4: Manual smoke test**

Run: `./target/release/foundry --help`
Expected: Shows help with all commands listed.

- [ ] **Step 5: Commit any remaining fixes**

If clippy or tests revealed issues, fix and commit.

- [ ] **Step 6: Final commit**

```bash
git add -A
git commit -m "chore: final cleanup and verification"
```
