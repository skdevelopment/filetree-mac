use filetree::fda::{
    build_fda_result, check_full_disk_access, fda_probe_paths, friendly_terminal_name,
    get_terminal_app_name, is_macos, open_fda_settings, probe_path,
};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

#[test]
fn test_is_macos() {
    assert_eq!(is_macos(), cfg!(target_os = "macos"));
}

#[test]
fn test_fda_probe_paths_defined() {
    assert!(fda_probe_paths().len() >= 2);
}

#[test]
fn test_check_full_disk_access_non_macos() {
    if cfg!(target_os = "macos") {
        return;
    }
    let result = check_full_disk_access();
    assert!(result.has_access);
    assert!(result.message.contains("macOS"));
}

#[test]
fn test_build_fda_result_blocked() {
    let result = build_fda_result("Warp", vec![PathBuf::from("/blocked")], 1);
    assert!(!result.has_access);
    assert!(!result.blocked_paths.is_empty());
    assert!(result.message.contains("Full Disk Access"));
    assert!(result.message.contains("Warp"));
    assert!(!result.inconclusive);
}

#[test]
fn test_build_fda_result_granted() {
    let result = build_fda_result("Terminal", vec![], 2);
    assert!(result.has_access);
    assert!(result.blocked_paths.is_empty());
    assert!(!result.inconclusive);
}

#[test]
fn test_build_fda_result_inconclusive() {
    let result = build_fda_result("Terminal", vec![], 0);
    assert!(result.has_access);
    assert!(result.inconclusive);
    assert!(result.message.contains("Could not verify"));
}

#[test]
fn test_friendly_terminal_name() {
    assert_eq!(friendly_terminal_name("Apple_Terminal"), "Terminal");
    assert_eq!(friendly_terminal_name("iTerm.app"), "iTerm");
    assert_eq!(friendly_terminal_name("Warp"), "Warp");
}

#[test]
fn test_get_terminal_app_name_env() {
    let prev = std::env::var("TERM_PROGRAM").ok();
    // SAFETY: test-local env override; restored before return.
    unsafe { std::env::set_var("TERM_PROGRAM", "Apple_Terminal") };
    assert_eq!(get_terminal_app_name(), "Terminal");
    unsafe { std::env::set_var("TERM_PROGRAM", "iTerm.app") };
    assert_eq!(get_terminal_app_name(), "iTerm");
    match prev {
        Some(v) => unsafe { std::env::set_var("TERM_PROGRAM", v) },
        None => unsafe { std::env::remove_var("TERM_PROGRAM") },
    }
}

#[test]
fn test_probe_path_file_permission() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("secret");
    fs::write(&file, "x").unwrap();
    let mut perms = fs::metadata(&file).unwrap().permissions();
    perms.set_mode(0o000);
    fs::set_permissions(&file, perms).unwrap();
    assert_eq!(probe_path(&file), Some(file));
}

#[test]
fn test_open_fda_settings_non_macos() {
    if cfg!(target_os = "macos") {
        return;
    }
    assert!(!open_fda_settings());
}

#[test]
fn test_fda_message_includes_terminal_name() {
    let result = build_fda_result("Warp", vec![PathBuf::from("/Library/Mail")], 1);
    assert!(!result.has_access);
    assert!(result.message.contains("Warp"));
}
