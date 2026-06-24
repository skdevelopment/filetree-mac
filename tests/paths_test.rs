use filetree::paths::{
    is_ancestor, is_delete_protected, is_under_scan_root, normalize_path, safe_delete_target,
    strip_private_prefix,
};
use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

#[test]
fn test_is_under_scan_root() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("scan");
    let child = root.join("child");
    fs::create_dir_all(&child).unwrap();
    assert!(is_under_scan_root(&child, &root));
    let outside = tmp.path().join("outside");
    fs::create_dir(&outside).unwrap();
    assert!(!is_under_scan_root(&outside, &root));
}

#[test]
fn test_delete_blocked_scan_root() {
    let tmp = tempfile::tempdir().unwrap();
    let scan_root = tmp.path().join("project");
    fs::create_dir(&scan_root).unwrap();
    let (protected, reason) = is_delete_protected(&scan_root, &scan_root);
    assert!(protected);
    assert!(reason.to_lowercase().contains("scan root"));
}

#[test]
fn test_delete_blocked_system_outside_scan_root() {
    let tmp = tempfile::tempdir().unwrap();
    let (protected, reason) = is_delete_protected(Path::new("/usr/local/bin"), tmp.path());
    assert!(protected);
    assert!(
        reason.to_lowercase().contains("outside") || reason.to_lowercase().contains("protected")
    );
}

#[test]
fn test_delete_blocked_home() {
    let tmp = tempfile::tempdir().unwrap();
    let home = fs::canonicalize(tmp.path().join("home")).unwrap_or_else(|_| {
        let p = tmp.path().join("home");
        fs::create_dir(&p).unwrap();
        fs::canonicalize(&p).unwrap()
    });
    let scan = tmp.path().join("scan");
    fs::create_dir(&scan).unwrap();
    let prev = std::env::var("HOME").ok();
    // SAFETY: test-local HOME override; restored before return.
    unsafe { std::env::set_var("HOME", home.as_os_str()) };
    let (protected, reason) = is_delete_protected(&home, &scan);
    match &prev {
        Some(p) => unsafe { std::env::set_var("HOME", p) },
        None => unsafe { std::env::remove_var("HOME") },
    }
    assert!(protected);
    assert!(reason.to_lowercase().contains("home"));
}

#[test]
fn test_delete_allowed_under_scan_root() {
    let tmp = tempfile::tempdir().unwrap();
    let scan_root = tmp.path().join("scan");
    let target = scan_root.join("cache");
    fs::create_dir_all(&target).unwrap();
    let (protected, reason) = is_delete_protected(&target, &scan_root);
    assert!(!protected, "{reason}");
}

#[test]
fn test_delete_blocked_ancestor_of_scan_root() {
    let tmp = tempfile::tempdir().unwrap();
    let ancestor = tmp.path().join("parent");
    let scan_root = ancestor.join("child");
    fs::create_dir_all(&scan_root).unwrap();
    let (protected, reason) = is_delete_protected(&ancestor, &scan_root);
    assert!(protected);
    assert!(reason.to_lowercase().contains("ancestor"));
}

#[test]
fn test_safe_delete_target_rejects_symlink() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("real.txt");
    fs::write(&target, "data").unwrap();
    let link = tmp.path().join("link.txt");
    symlink(&target, &link).unwrap();
    let (_, error) = safe_delete_target(&link, tmp.path(), false);
    assert!(error.is_some());
    assert!(error.unwrap().to_lowercase().contains("symlink"));
}

#[test]
fn test_safe_delete_target_missing_path() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("missing");
    let (_, error) = safe_delete_target(&missing, tmp.path(), false);
    assert!(error.is_some());
    assert!(error.unwrap().to_lowercase().contains("exist"));
}

#[test]
fn test_safe_delete_target_type_mismatch() {
    let tmp = tempfile::tempdir().unwrap();
    let file_path = tmp.path().join("file.txt");
    fs::write(&file_path, "x").unwrap();
    let (_, error) = safe_delete_target(&file_path, tmp.path(), true);
    assert!(error.is_some());
    assert!(error.unwrap().to_lowercase().contains("directory"));
}

#[test]
fn test_safe_delete_target_returns_canonical_path() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("file.txt");
    fs::write(&file, "x").unwrap();
    let (target, error) = safe_delete_target(&file, tmp.path(), false);
    assert!(error.is_none());
    assert_eq!(target, normalize_path(&file));
}

#[test]
fn test_is_ancestor() {
    let tmp = tempfile::tempdir().unwrap();
    let ancestor = tmp.path().join("parent");
    let descendant = ancestor.join("child");
    fs::create_dir_all(&descendant).unwrap();
    assert!(is_ancestor(&ancestor, &descendant));
    assert!(!is_ancestor(&descendant, &ancestor));
    assert!(!is_ancestor(&ancestor, &ancestor));
}

#[test]
fn test_normalize_path_resolves_symlink() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("real");
    fs::create_dir(&target).unwrap();
    let link = tmp.path().join("link");
    symlink(&target, &link).unwrap();
    assert_eq!(normalize_path(&link), normalize_path(&target));
}

#[test]
fn test_strip_private_prefix() {
    let p = strip_private_prefix(std::path::Path::new("/private/var/tmp"));
    assert_eq!(p, std::path::Path::new("/var/tmp"));
}
