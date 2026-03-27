use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
    /// Allocated ports for this workspace (env var name -> port number)
    #[serde(default)]
    pub allocated_ports: HashMap<String, u16>,
    /// PR number if a PR was created via `foundry pr` (None = no PR, use local merge)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_number: Option<u64>,
    /// PR URL if a PR was created via `foundry pr`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_url: Option<String>,
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

    pub fn add(&mut self, workspace: Workspace) {
        // Remove any existing entry with the same project+name to prevent duplicates
        self.inner
            .workspaces
            .retain(|w| !(w.project == workspace.project && w.name == workspace.name));
        self.inner.workspaces.push(workspace);
    }

    pub fn remove(&mut self, project: &str, name: &str) {
        self.inner
            .workspaces
            .retain(|w| !(w.project == project && w.name == name));
    }

    pub fn list(&self) -> &[Workspace] {
        &self.inner.workspaces
    }

    pub fn find_by_project(&self, project: &str) -> Vec<&Workspace> {
        self.inner
            .workspaces
            .iter()
            .filter(|w| w.project == project)
            .collect()
    }

    pub fn find_by_worktree_path(&self, path: &str) -> Option<&Workspace> {
        let cwd = Path::new(path);
        self.inner.workspaces.iter().find(|w| {
            // Use Path::starts_with which compares by path components.
            // This handles mixed separators on Windows (e.g., stored path
            // has forward slashes from config while cwd uses backslashes).
            cwd.starts_with(Path::new(&w.worktree_path))
        })
    }

    /// Record that a PR was created for this workspace.
    pub fn set_pr_info(&mut self, project: &str, name: &str, pr_number: u64, pr_url: &str) {
        if let Some(ws) = self
            .inner
            .workspaces
            .iter_mut()
            .find(|w| w.project == project && w.name == name)
        {
            ws.pr_number = Some(pr_number);
            ws.pr_url = Some(pr_url.to_string());
        }
    }

    /// Clear PR info (e.g., after --local merge ignoring a closed PR).
    pub fn clear_pr_info(&mut self, project: &str, name: &str) {
        if let Some(ws) = self
            .inner
            .workspaces
            .iter_mut()
            .find(|w| w.project == project && w.name == name)
        {
            ws.pr_number = None;
            ws.pr_url = None;
        }
    }

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

    pub fn prune_stale(&mut self) {
        self.inner
            .workspaces
            .retain(|w| Path::new(&w.worktree_path).exists());

        // Deduplicate by project+name, keeping the last entry (most recent)
        let mut seen = std::collections::HashSet::new();
        let mut deduped = Vec::new();
        for ws in self.inner.workspaces.iter().rev() {
            let key = format!("{}:{}", ws.project, ws.name);
            if seen.insert(key) {
                deduped.push(ws.clone());
            }
        }
        deduped.reverse();
        self.inner.workspaces = deduped;
    }

    /// Get all ports currently allocated across all active workspaces.
    pub fn all_allocated_ports(&self) -> Vec<u16> {
        self.inner
            .workspaces
            .iter()
            .flat_map(|w| w.allocated_ports.values().copied())
            .collect()
    }
}
