use anyhow::{Context, Result, bail};
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

    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let contents =
            toml::to_string_pretty(&self.inner).context("failed to serialize registry")?;
        std::fs::write(path, contents)
            .with_context(|| format!("failed to write {}", path.display()))?;
        Ok(())
    }

    pub fn add(&mut self, name: &str, path: PathBuf) -> Result<()> {
        if self.inner.projects.contains_key(name) {
            bail!(
                "project '{name}' already exists. Use `foundry projects remove` first, or choose a different name."
            );
        }
        self.inner
            .projects
            .insert(name.to_string(), ProjectEntry { path });
        Ok(())
    }

    pub fn remove(&mut self, name: &str) -> Result<()> {
        if self.inner.projects.remove(name).is_none() {
            bail!("project '{name}' not found");
        }
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<PathBuf> {
        self.inner.projects.get(name).map(|e| e.path.clone())
    }

    pub fn list(&self) -> Vec<(String, PathBuf)> {
        self.inner
            .projects
            .iter()
            .map(|(k, v)| (k.clone(), v.path.clone()))
            .collect()
    }

    pub fn find_by_path(&self, path: &Path) -> Option<String> {
        self.inner
            .projects
            .iter()
            .find(|(_, v)| v.path == path)
            .map(|(k, _)| k.clone())
    }
}
