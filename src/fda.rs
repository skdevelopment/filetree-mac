use std::path::{Path, PathBuf};
use std::process::Command;

pub const FDA_SETTINGS_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles";

pub fn fda_probe_paths() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    vec![
        home.join("Library/Safari"),
        PathBuf::from("/Library/Application Support/com.apple.TCC/TCC.db"),
        home.join("Library/Mail"),
        home.join("Library/Application Support"),
    ]
}

pub fn fda_fallback_probe() -> PathBuf {
    std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join("Library"))
        .unwrap_or_else(|| PathBuf::from("~/Library"))
}

#[derive(Debug, Clone)]
pub struct FdaResult {
    pub has_access: bool,
    pub blocked_paths: Vec<PathBuf>,
    pub message: String,
    pub inconclusive: bool,
}

pub fn is_macos() -> bool {
    cfg!(target_os = "macos")
}

pub fn probe_path(path: &Path) -> Option<PathBuf> {
    if !path.exists() {
        return None;
    }
    if path.is_file() {
        match std::fs::File::open(path) {
            Ok(mut f) => {
                use std::io::Read;
                let mut buf = [0u8; 1];
                match f.read(&mut buf) {
                    Ok(_) => None,
                    Err(e) if is_permission_error(&e) => Some(path.to_path_buf()),
                    Err(_) => None,
                }
            }
            Err(e) if is_permission_error_io(&e) => Some(path.to_path_buf()),
            Err(_) => None,
        }
    } else {
        match std::fs::read_dir(path) {
            Ok(_) => None,
            Err(e) if is_permission_error_io(&e) => Some(path.to_path_buf()),
            Err(_) => None,
        }
    }
}

fn is_permission_error(e: &std::io::Error) -> bool {
    e.kind() == std::io::ErrorKind::PermissionDenied
        || e.raw_os_error() == Some(libc::EPERM)
        || e.raw_os_error() == Some(libc::EACCES)
}

fn is_permission_error_io(e: &std::io::Error) -> bool {
    is_permission_error(e)
}

pub fn check_full_disk_access() -> FdaResult {
    if !is_macos() {
        return FdaResult {
            has_access: true,
            blocked_paths: vec![],
            message: "Full Disk Access check is only required on macOS.".to_string(),
            inconclusive: false,
        };
    }

    let terminal = get_terminal_app_name();
    let mut blocked = Vec::new();
    let mut probes_attempted = 0u32;

    for path in fda_probe_paths() {
        if !path.exists() {
            continue;
        }
        probes_attempted += 1;
        if let Some(b) = probe_path(&path) {
            blocked.push(b);
        }
    }

    if probes_attempted == 0 {
        let fallback = fda_fallback_probe();
        if let Some(b) = probe_path(&fallback) {
            blocked.push(b);
        } else if fallback.exists() {
            probes_attempted = 1;
        }
    }

    build_fda_result(&terminal, blocked, probes_attempted)
}

/// Build an FDA result from probe outcomes (used by tests and `check_full_disk_access`).
pub fn build_fda_result(terminal: &str, blocked: Vec<PathBuf>, probes_attempted: u32) -> FdaResult {
    if !blocked.is_empty() {
        return FdaResult {
            has_access: false,
            blocked_paths: blocked,
            message: format!(
                "filetree does not have Full Disk Access. Some folders will show \
                 as empty or inaccessible.\n\n\
                 To grant access:\n\
                   1. Open System Settings → Privacy & Security → Full Disk Access\n\
                   2. Click + and add {terminal}\n\
                   3. Enable the toggle for {terminal}\n\
                   4. Restart the terminal and run filetree again\n\n\
                 Press 'o' in the FDA dialog to open System Settings."
            ),
            inconclusive: false,
        };
    }

    if probes_attempted == 0 {
        return FdaResult {
            has_access: true,
            blocked_paths: vec![],
            inconclusive: true,
            message: "Could not verify Full Disk Access (no probe paths available). \
                      Some protected folders may still be inaccessible.\n\n\
                      You can grant FDA in System Settings, or choose Continue to scan anyway."
                .to_string(),
        };
    }

    FdaResult {
        has_access: true,
        blocked_paths: vec![],
        message: "Full Disk Access appears to be granted.".to_string(),
        inconclusive: false,
    }
}

pub fn open_fda_settings() -> bool {
    if !is_macos() {
        return false;
    }
    Command::new("open")
        .arg(FDA_SETTINGS_URL)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

const TERMINAL_DISPLAY_NAMES: &[(&str, &str)] = &[
    ("Apple_Terminal", "Terminal"),
    ("iTerm.app", "iTerm"),
    ("Warp", "Warp"),
    ("vscode", "VS Code"),
    ("Cursor", "Cursor"),
];

pub fn friendly_terminal_name(raw: &str) -> String {
    for (k, v) in TERMINAL_DISPLAY_NAMES {
        if raw == *k {
            return (*v).to_string();
        }
    }
    if raw == "Terminal" || raw.ends_with(".Terminal") {
        return "Terminal".to_string();
    }
    raw.replace('_', " ")
}

pub fn get_terminal_app_name() -> String {
    if let Ok(term) = std::env::var("TERM_PROGRAM") {
        if !term.is_empty() {
            return friendly_terminal_name(&term);
        }
    }
    if let Ok(bundle) = std::env::var("__CFBundleIdentifier") {
        if let Some(last) = bundle.split('.').next_back() {
            return friendly_terminal_name(last);
        }
    }
    "your terminal application".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_friendly_terminal_name() {
        assert_eq!(friendly_terminal_name("Apple_Terminal"), "Terminal");
        assert_eq!(friendly_terminal_name("iTerm.app"), "iTerm");
        assert_eq!(friendly_terminal_name("Warp"), "Warp");
    }

    #[test]
    fn test_build_fda_result_blocked() {
        let result = build_fda_result("Warp", vec![PathBuf::from("/blocked")], 1);
        assert!(!result.has_access);
        assert!(result.message.contains("Warp"));
        assert!(!result.inconclusive);
    }

    #[test]
    fn test_build_fda_result_inconclusive() {
        let result = build_fda_result("Terminal", vec![], 0);
        assert!(result.has_access);
        assert!(result.inconclusive);
    }
}
