use filetree::platform::default_scan_path;
use std::path::{Path, PathBuf};

#[test]
fn test_default_scan_path_macos() {
    if cfg!(target_os = "macos") {
        assert_eq!(default_scan_path(), PathBuf::from("/"));
    }
}

#[test]
fn test_default_scan_path_non_macos() {
    if !cfg!(target_os = "macos") {
        let path = default_scan_path();
        assert!(path == Path::new("~") || path.starts_with("/"));
    }
}
