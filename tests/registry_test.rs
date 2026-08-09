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

#[test]
fn test_find_by_path_exact_match() {
    let dir = TempDir::new().unwrap();
    let registry_path = dir.path().join("projects.toml");
    let mut registry = foundry::registry::Registry::load_from(&registry_path).unwrap();
    registry.add("myapp", dir.path().to_path_buf()).unwrap();
    assert_eq!(registry.find_by_path(dir.path()).as_deref(), Some("myapp"));
}

#[test]
fn test_find_by_path_no_match() {
    let dir = TempDir::new().unwrap();
    let other = TempDir::new().unwrap();
    let registry_path = dir.path().join("projects.toml");
    let mut registry = foundry::registry::Registry::load_from(&registry_path).unwrap();
    registry.add("myapp", dir.path().to_path_buf()).unwrap();
    assert!(registry.find_by_path(other.path()).is_none());
}

/// `projects add` stores a canonicalized path while `resolve_project` supplies
/// whatever git reports, so the two spellings of one directory have to match.
/// A raw `==` did not, and the miss fell through to auto-registration, which
/// then bailed with "already registered to a different path".
#[cfg(unix)]
#[test]
fn test_find_by_path_matches_through_a_symlink() {
    let real = TempDir::new().unwrap();
    let link_parent = TempDir::new().unwrap();
    let link = link_parent.path().join("link-to-project");
    std::os::unix::fs::symlink(real.path(), &link).unwrap();

    let registry_path = real.path().join("projects.toml");
    let mut registry = foundry::registry::Registry::load_from(&registry_path).unwrap();

    // Registered by its real path, as `projects add` canonicalizes it...
    registry
        .add("myapp", std::fs::canonicalize(real.path()).unwrap())
        .unwrap();
    // ...and looked up through the symlink.
    assert_eq!(registry.find_by_path(&link).as_deref(), Some("myapp"));
}

/// A registered path whose repository has since been deleted must not break
/// lookups for the projects that are still there.
#[test]
fn test_find_by_path_tolerates_a_stale_entry() {
    let dir = TempDir::new().unwrap();
    let registry_path = dir.path().join("projects.toml");
    let mut registry = foundry::registry::Registry::load_from(&registry_path).unwrap();
    registry
        .add("gone", PathBuf::from("/no/such/directory/anywhere"))
        .unwrap();
    registry.add("here", dir.path().to_path_buf()).unwrap();
    assert_eq!(registry.find_by_path(dir.path()).as_deref(), Some("here"));
}
