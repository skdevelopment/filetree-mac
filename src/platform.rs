use std::path::PathBuf;

/// Return the default path to scan on this platform (whole system disk on macOS).
pub fn default_scan_path() -> PathBuf {
    if cfg!(target_os = "macos") {
        PathBuf::from("/")
    } else {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("~"))
    }
}
