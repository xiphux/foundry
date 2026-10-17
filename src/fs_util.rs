use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Create (if needed) and return `~/.foundry/run/<name>`, owner-only.
///
/// Runtime scratch files — the Zellij layout, the Windows Terminal pid files —
/// used to live at a predictable path under `std::env::temp_dir()`. On Linux
/// that is `/tmp`, mode 1777, and the names were fully predictable: the
/// workspace path hashed with `DefaultHasher`, which is seeded with fixed keys
/// and so is stable across processes and runs. Any local user could therefore
/// pre-create the file as a symlink and have foundry write through it, or win
/// the race between foundry writing the file and the terminal reading it and
/// substitute their own contents. Both files are then acted on: the layout
/// names commands for Zellij to run, and the pid files name processes to kill.
///
/// `~/.foundry` is already the private per-user directory for foundry's state,
/// so putting runtime files there removes the shared-directory problem
/// entirely rather than trying to write into `/tmp` safely.
pub fn runtime_subdir(name: &str) -> Result<PathBuf> {
    let run_dir = crate::config::foundry_dir()?.join("run");
    create_private_dir(&run_dir)?;
    let dir = run_dir.join(name);
    create_private_dir(&dir)?;
    Ok(dir)
}

/// Make the default foundry home, `~/.foundry`, owner-only, creating it if need be.
///
/// Its files hold nothing secret, but the agent status files carry the agent's
/// last message and the context files its instructions, which can quote the
/// code being worked on — not something to leave world-readable on a shared
/// machine. Only the default location: a `FOUNDRY_HOME` the user chose is
/// theirs to set permissions on.
pub fn secure_default_home() -> Result<()> {
    if std::env::var_os("FOUNDRY_HOME").is_some_and(|d| !d.is_empty()) {
        return Ok(());
    }
    create_private_dir(&crate::config::foundry_dir()?)
}

/// Create a directory that only its owner may access.
///
/// The mode is applied after creation rather than relying on the process
/// umask, and re-applied to an existing directory whose permissions are too
/// broad — otherwise a directory created by an older version, or by a user
/// with a permissive umask, would silently stay group- or world-accessible.
fn create_private_dir(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("failed to create {}", dir.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(dir)
            .with_context(|| format!("failed to stat {}", dir.display()))?
            .permissions();
        if perms.mode() & 0o077 != 0 {
            perms.set_mode(0o700);
            std::fs::set_permissions(dir, perms)
                .with_context(|| format!("failed to restrict permissions on {}", dir.display()))?;
        }
    }

    Ok(())
}

/// Canonicalize as much of `path` as still exists, keeping the rest verbatim.
///
/// A plain `canonicalize` fails outright on a path that no longer exists, which
/// would leave an entry unrevocable once its repository is deleted — the case
/// that most needs revoking. On macOS that bites even for a path the user
/// typed correctly, because the key was stored through the `/var` ->
/// `/private/var` symlink and resolving it again requires the directory to
/// still be there.
pub fn canonicalize_existing_prefix(path: &Path) -> PathBuf {
    if let Ok(resolved) = std::fs::canonicalize(path) {
        return resolved;
    }

    // Walk up until something resolves, then re-append what was trimmed.
    let mut trimmed: Vec<std::ffi::OsString> = Vec::new();
    let mut cursor = path;
    while let (Some(parent), Some(name)) = (cursor.parent(), cursor.file_name()) {
        trimmed.push(name.to_os_string());
        if let Ok(base) = std::fs::canonicalize(parent) {
            return trimmed.iter().rev().fold(base, |mut acc, part| {
                acc.push(part);
                acc
            });
        }
        cursor = parent;
    }

    path.to_path_buf()
}

/// Write `contents` to `path` atomically.
///
/// Writes to a temporary file in the same directory and renames it over the
/// target. A plain `fs::write` truncates first, leaving a window where a
/// concurrent reader sees a partial — or empty — file. That matters here
/// because `status --watch` re-reads `state.toml` every two seconds and both
/// state and registry files deserialize an empty file as "no entries" rather
/// than failing.
///
/// The temporary name includes the process id so two foundry processes saving
/// at once cannot write to each other's scratch file; without that, one
/// process's rename can publish another's half-written content, and the loser
/// fails with a confusing "no such file" error.
pub fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let extension = match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => format!("{ext}.tmp.{}", std::process::id()),
        None => format!("tmp.{}", std::process::id()),
    };
    let tmp = path.with_extension(extension);

    std::fs::write(&tmp, contents).with_context(|| format!("failed to write {}", tmp.display()))?;

    // Rename within a directory is atomic, so a reader sees old or new, never
    // a mix. On failure the scratch file is ours alone, so cleaning it up
    // cannot disturb another process.
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("failed to replace {}", path.display()));
    }

    Ok(())
}

/// Whether `inner` lies inside `outer`, comparing what the two paths resolve to.
///
/// The spelling cannot be trusted here, because the two sides are built
/// differently by construction: `current_dir` returns what the kernel resolved,
/// while a worktree path is the recorded `worktree_dir` join, tilde-expanded and
/// nothing more. One symlinked component anywhere in `worktree_dir` — macOS's
/// `$TMPDIR` under `/var`, a symlinked home or volume — made a raw comparison
/// answer "not inside" for a process that *was* inside, which is the answer that
/// closes the terminal tab out from under the cleanup running in it.
pub fn path_contains(outer: &Path, inner: &Path) -> bool {
    canonicalize_existing_prefix(inner).starts_with(canonicalize_existing_prefix(outer))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// The guard that keeps cleanup from closing the tab it is running in has to
    /// see through a symlinked ancestor — the everyday case is a `worktree_dir`
    /// under macOS's `$TMPDIR`, which is reached via `/var` -> `/private/var`.
    #[cfg(unix)]
    #[test]
    fn path_contains_sees_through_a_symlinked_ancestor() {
        let dir = TempDir::new().unwrap();
        let real = dir.path().join("real");
        let worktree = real.join("wt");
        std::fs::create_dir_all(&worktree).unwrap();
        std::os::unix::fs::symlink(&real, dir.path().join("link")).unwrap();

        // The same worktree spelled through the symlink, as foundry's recorded
        // path would be, against the resolved cwd the kernel reports.
        let spelled_through_link = dir.path().join("link").join("wt");
        assert!(path_contains(
            &spelled_through_link,
            &worktree.canonicalize().unwrap()
        ));
        assert!(path_contains(&spelled_through_link, &worktree.join("src")));
    }

    /// A sibling whose name merely starts with the same characters is not inside.
    #[test]
    fn path_contains_does_not_match_a_name_prefix() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir(dir.path().join("wt")).unwrap();
        std::fs::create_dir(dir.path().join("wt-other")).unwrap();

        assert!(!path_contains(
            &dir.path().join("wt"),
            &dir.path().join("wt-other")
        ));
    }

    #[test]
    fn writes_contents_to_target() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("state.toml");
        write_atomic(&path, "hello").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
    }

    #[test]
    fn overwrites_existing_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("state.toml");
        std::fs::write(&path, "old").unwrap();
        write_atomic(&path, "new").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
    }

    #[test]
    fn creates_missing_parent_directories() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nested").join("deep").join("state.toml");
        write_atomic(&path, "x").unwrap();
        assert!(path.exists());
    }

    /// The scratch file must not survive a successful write.
    #[test]
    fn leaves_no_temporary_file_behind() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("state.toml");
        write_atomic(&path, "x").unwrap();

        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n != "state.toml")
            .collect();
        assert!(leftovers.is_empty(), "stray files: {leftovers:?}");
    }

    /// Runtime scratch must never be readable by other users on the box.
    #[cfg(unix)]
    #[test]
    fn runtime_subdir_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new().unwrap();
        let nested = dir.path().join("run").join("zellij-layouts");
        create_private_dir(&nested).unwrap();

        let mode = std::fs::metadata(&nested).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700, "got mode {:o}", mode & 0o777);
    }

    /// A directory left group- or world-accessible by an earlier version must
    /// be tightened, not accepted as-is.
    #[cfg(unix)]
    #[test]
    fn create_private_dir_tightens_an_existing_loose_directory() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new().unwrap();
        let loose = dir.path().join("loose");
        std::fs::create_dir_all(&loose).unwrap();
        std::fs::set_permissions(&loose, std::fs::Permissions::from_mode(0o777)).unwrap();

        create_private_dir(&loose).unwrap();

        let mode = std::fs::metadata(&loose).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700, "got mode {:o}", mode & 0o777);
    }

    /// Two processes must not share a scratch path.
    #[test]
    fn temporary_name_is_process_specific() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("state.toml");
        let extension = format!("toml.tmp.{}", std::process::id());
        assert_eq!(
            path.with_extension(extension).file_name().unwrap(),
            std::ffi::OsStr::new(&format!("state.toml.tmp.{}", std::process::id()))
        );
    }

    /// A reader polling during repeated writes must never observe an empty or
    /// partial file — that was the flash of "No active workspaces" in --watch.
    #[test]
    fn concurrent_reader_never_sees_partial_content() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("state.toml");
        let body = "workspace = \"x\"\n".repeat(4000);
        write_atomic(&path, &body).unwrap();

        let reader_path = path.clone();
        let expected_len = body.len();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let reader_stop = stop.clone();

        let reader = std::thread::spawn(move || {
            let mut bad = 0;
            while !reader_stop.load(std::sync::atomic::Ordering::Relaxed) {
                if let Ok(s) = std::fs::read_to_string(&reader_path)
                    && s.len() != expected_len
                {
                    bad += 1;
                }
            }
            bad
        });

        for _ in 0..200 {
            write_atomic(&path, &body).unwrap();
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);

        assert_eq!(reader.join().unwrap(), 0, "reader saw a partial file");
    }
}
