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
